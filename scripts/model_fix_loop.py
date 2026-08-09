#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Close oxidex/ExifTool tag-coverage gaps via any OpenAI-compatible model API.

Config lives in config.toml (see config.example.toml), not environment
variables. Each of the [worker] and [reviewer] tables takes:

    base_url            e.g. "https://api.z.ai/api/paas/v4"  (GLM-5.2)
    api_key
    models               list of model names, e.g. ["glm-5.2", "glm-5.2-fast"]
                          -- one is picked at random for every individual
                          model call (fixer attempt or reviewer verdict), so
                          a run rotates across the whole pool rather than
                          pinning to one model. Each entry may also set
                          phase = "explore"|"patch" (which conversation
                          turns it serves -- see models_for_phase) and
                          reasoning_effort (per-model override of the
                          table default).
    max_tokens           default 4096 (cap on the model's own reply length --
                          if the model's own diff would exceed this in one
                          reply, the prompt tells it to split into
                          "PATCH i/N" chunks instead; see attempt_build and
                          build_reply_shape_manifest. This is the REPLY cap,
                          not max_prompt_tokens below -- the two used to be
                          conflated in the manifest text handed to the model,
                          which quoted max_prompt_tokens (the INPUT budget,
                          usually double this) as the PATCH-chunking
                          threshold, so a diff sized to fit under the real
                          cap still looked "safely under budget" to the
                          model and got silently truncated mid-diff instead.)
    max_prompt_tokens     default 8192 (worker only; hard cap on the built
                          prompt itself -- see estimate_tokens/
                          assemble_prompt_sections -- a ~4 chars/token
                          estimate, no real tokenizer dependency. Overflow
                          is shed via graduated per-section truncation
                          (attempts, then samples, then neighbor precedent,
                          then perl_block, then parser files down to
                          parser_floor_tokens), not plain head-keeping.)
    reviewer_max_prompt_tokens default 8192 (reviewer only; independent cap
                          on build_review_prompt's own prompt, which now
                          carries the Perl reference, live post-fix
                          evidence, and a scoped emission scan alongside
                          the C1-C5 checklist -- see review_verdict)
    learning_budget_tokens default 1200 (worker only; flat, reserved token
                          budget for the learning block -- adaptive
                          diff-format remediation + this worker's own
                          quarantine verdicts + sweep reviews + module
                          playbook + lessons tail -- never squeezed further
                          and never dropped entirely. Overflow inside the
                          block is shed from the tail of
                          LEARNING_SECTION_ORDER, so a growing ledger costs
                          the low-value sections first, never the
                          high-value ones. See build_prompt /
                          compose_learning_block. The GLOBAL-PITFALLS.md
                          excerpt is outside this budget -- it sits in the
                          static cacheable prefix.)
    parser_floor_tokens    default 2000 (worker only; the parser-files
                          section never shrinks below this even under the
                          worst prompt overflow -- "elastic with a floor")
    lessons_tail_kb        default 256 (worker only; how far back
                          build_prompt seeks into the tail of
                          logs/lessons.jsonl -- bounded, no full scan of a
                          ledger that only grows -- see
                          read_lessons_tail_events. Those bytes are read
                          ONCE and feed both the recurrence-ranked
                          module/format lessons and the per-worker
                          "no diff in model response" detector.)
    max_request_repeats    default 3 (worker only; identical REQUESTs before
                          a pivot nudge replaces the served content)
    max_verify_turns       default 10 (worker only; VERIFY trial-compile
                          turns per attempt -- see attempt_build)
    compaction_trigger_tokens      default 12000; conversation size (est.
                          tokens) beyond which stale served payloads are
                          stubbed -- see compact_messages
    compaction_keep_recent_turns   default 4; most-recent messages exempt
                          from compaction
    compaction_min_elide_tokens    default 3000; a served user payload is
                          only stubbed when its own estimated tokens
                          exceed this floor -- smaller turns are left intact
    reasoning_effort      default "max"
    max_prompt_tags       default 40 (worker only; per-attempt cap on
                          missing_tags/value_differences shown -- the rest
                          resurface in later rounds automatically)
    max_prompt_file_bytes default 60000 (worker only; per-attempt cap on
                          total parser-file source bytes included)
    stream                default true; requests the response as
                          OpenAI-compatible SSE and reassembles it into the
                          same full-string reply either way. When streaming,
                          stream_options.include_usage is set so the provider
                          still returns token/cache accounting in a final
                          usage-only chunk.
    prompt_cache          default "auto"; "auto" relies on the provider's
                          automatic prefix caching (build_prompt orders
                          sections static-first to maximise the stable
                          prefix), "explicit" additionally wraps that prefix
                          in an Anthropic-style cache_control breakpoint
                          (opt-in -- only helps providers that accept it),
                          "off" disables both. Cached-token counts the
                          provider reports are written to cache-stats.log.
    thinking               default true; false sends
                          "thinking": {"type": "disabled"} in the request
                          body. True omits the field entirely (the API's own
                          default), rather than guessing at an "enabled"
                          shape the docs don't show.
    temperature            default 0 (deterministic)
    timeout                default 120 (socket read timeout in seconds --
                          some providers hold a streaming connection open
                          with keepalives well past this before ever
                          sending real content, so raise it if a provider
                          is otherwise reliable but just slow)
    deadline_seconds       default 120 (wall-clock ceiling on ONE model
                          call, checked as stream chunks arrive). Unlike
                          timeout, arriving data does NOT reset it, so
                          this is what actually bounds a call: a provider
                          trickling SSE chunks satisfies timeout forever
                          (measured 2118s against timeout=1200). On expiry
                          the connection is closed and the request is
                          replayed, counting against max_retries.
    max_request_turns      default 20 (worker only; how many UNPRODUCTIVE
                          REQUEST: <path> turns -- a path that doesn't
                          resolve, a range past the end, or a re-ask of
                          content still visible in the conversation -- the
                          fixer gets before it's nudged, then required, to
                          submit a diff. REQUESTs that actually serve it
                          something new are FREE and unlimited; see
                          attempt_build/resolve_request/request_answer_served)
    max_request_turns_ceiling  default 250 (worker only; runaway backstop on
                          TOTAL REQUESTs per attempt, free ones included --
                          reached only by a loop, named to the model only
                          when it fires)
    max_review_rounds      default 5 (worker only; EXTRA fix_gap rounds, on
                          top of max_repair_rounds, that only a substantive
                          reviewer rejection can unlock -- one per
                          rejection, none for a reviewer outage or an
                          unparseable verdict. A fix
                          the reviewer keeps arguing with gets 5 + 5 = 10
                          rounds of back-and-forth; one that won't compile
                          still gets 5. See DEFAULT_MAX_REVIEW_ROUNDS.)
    max_retries            default 1000 (retries on a transient upstream
                          failure -- 5xx HTTPError, a connection-level
                          URLError (DNS/refused/TLS/stalled read), or a
                          completely empty reply -- before giving up on
                          one model call; high, not unlimited, to ride
                          out a long outage rather than blacklist a tag
                          over infrastructure being down)
    retry_backoff_seconds  default 2 (first retry's delay; doubles each
                          subsequent retry)
    max_retry_backoff_seconds default 120 (caps the exponential backoff's
                          growth -- otherwise a large max_retries implies
                          an absurd wait on later attempts)
    governor_calls_per_minute default 30 (cross-process rate governor:
                          steady-state model-call budget shared by every
                          worker through one flock-guarded token bucket
                          at ~/.oxidex/logs/rate-governor.json -- see
                          governor_acquire. The governor is
                          account-global, so the [worker] table's knobs
                          govern every phase, reviewer calls included)
    governor_burst         default 5 (bucket capacity -- calls that may
                          go out back-to-back before the per-minute
                          refill rate throttles the rest)
                          (governor_cooldown_seconds and
                          governor_max_cooldown_seconds are gone: the
                          fleet-wide cooldown they configured pauses every
                          worker over one worker's 429, and is replaced by
                          per-worker jittered retry plus a process-local
                          per-endpoint park -- see call_model. A config
                          still setting them is simply not read.)
    max_cluster_tags       default 6 (worker only; sibling-tag
                          clustering -- the selected tag pulls up to
                          max_cluster_tags - 1 still-active sibling tags
                          (same format/family/parser files) into one fix
                          conversation -- see choose_next_gap. 1 restores
                          the old one-tag-per-conversation behavior)
    use_sccache            default true; false sets OXIDEX_USE_SCCACHE=0
                          so cargo_env never routes rustc through
                          sccache (cargo's normal per-worktree
                          incremental cache only)
    claim_stale_seconds    default 7200 (worker only; a tag claim in the
                          shared tag-state older than this is treated as
                          abandoned -- its owner is dead, since a live
                          owner's heartbeat re-stamps it -- and may be
                          re-claimed by any worker; see run_tag_loop)
    heartbeat_seconds      default 60 (worker only; cadence at which a
                          daemon thread re-stamps this worker's claim
                          while an attempt is in flight, so long
                          governor waits and cargo runs never let a
                          live claim go stale; 0 disables)

[reviewer] defaults to [worker] entirely when omitted, so a single table
covers both the fixer and the reviewer by default -- add [reviewer] only to
run review on a different model pool/provider.

An optional [parallel] table configures scripts/parallel_tag_fix_loop.py:

    workers                default 4 -- number of concurrent worker
                          processes, each in its own persistent worktree
    max_tags_per_process   default 1 -- stop a worker after it has
                          attempted this many distinct tags, rather than
                          running until the whole shared tag pool is
                          blacklisted/fixed. Respawning frequently (rather
                          than one worker grinding through many tags on a
                          long-lived private branch) is what makes real
                          progress land on the shared branch often.

Usage:
    uv run scripts/model_fix_loop.py
    uv run scripts/model_fix_loop.py --only-format JPEG
    uv run scripts/model_fix_loop.py --config /path/to/config.toml
"""
import argparse
import datetime
import difflib
import email.utils
import fcntl
import functools
import json
import os
import random
import re
import shutil
import signal
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import tempfile
import threading
import time
import tomllib
import urllib.error
import urllib.request
from contextlib import contextmanager
from pathlib import Path

from find_tag_gaps import (
    DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS,
    DEFAULT_BUILD_SEMAPHORE_PATH,
    OXIDEX_HOME,
    REPO_ROOT,
    build_semaphore,
    group_gaps_by_format,
    load_comparison_report,
    run_format_comparison,
    run_full_comparison,
)

# distill_lessons.py is the canonical K1 owner: the event schema/enum,
# reason normalization, fingerprint_scoped/fingerprint_generic, and the
# O_APPEND/2000-byte-clamp append helper. This module is a WRITER (see K1
# call sites in critique_and_continue/fix_gap below) -- it delegates
# rather than keeping its own copy, exactly like log_sweep_review.py does.
from distill_lessons import (
    append_lesson as _dl_append_lesson,
    is_infra_reason,
    make_lesson as make_lesson_event,
    rank_by_recurrence,
)

# validate_fix_commit.py is the canonical owner of "which squad does this
# worker id belong to" -- it is the helper the merger itself uses when it
# decides ownership of a candidate commit (see check_ownership). Reusing
# it, rather than keeping a second regex here, is what guarantees the
# prompt and the validator can never disagree about whose commit was
# rejected. Same sibling-import shape as find_tag_gaps/distill_lessons
# above; validate_fix_commit is stdlib-only and imports nothing from here.
from validate_fix_commit import squad_from_worker

# exiftool_oracle.py is the canonical owner of "which ExifTool do we grade
# against". Never invoke a bare `exiftool`: PATH resolved to 13.55 while the
# tables are transcribed from 13.59, and the two disagree about which
# sub-table a given byte count selects. Same sibling-import shape as above.
import exiftool_oracle
from exiftool_oracle import shared as shared_exiftool_oracle

# --- K1 lessons ledger (writer side; see distill_lessons.py for the ---------
# --- canonical schema/fingerprint/append contract) --------------------------

def append_lesson(home_or_path, event):
    """Thin K1 wrapper delegating to distill_lessons.append_lesson (the
    canonical owner of the event schema, fingerprints, and the
    O_APPEND/2000-byte-clamp write contract) -- a sibling import exactly
    like find_tag_gaps above.

    home_or_path may be an OXIDEX_HOME-style directory (the usual case:
    "<home>/logs/lessons.jsonl" is resolved for you) or an already-full
    path ending in "lessons.jsonl" -- tests and callers that already have
    the exact ledger path don't have to fight a home-relative convention.

    Every call site in this module wraps this in try/except OSError: a
    lesson-append failure is best-effort observability and must NEVER
    break the fixer loop (K1 writers, item 2 of the Phase-1 spec)."""
    path = Path(home_or_path)
    if path.name != "lessons.jsonl":
        path = path / "logs" / "lessons.jsonl"
    return _dl_append_lesson(path, event)


DIFF_BLOCK_RE = re.compile(r"```diff[ \t]*\r?\n(.*?)```", re.DOTALL)


#: Fallback for a ```diff fence that opens and never closes -- see
#: extract_diff. Only tried once DIFF_BLOCK_RE has already failed to find a
#: closing ``` anywhere in the reply, so a well-formed fenced block is always
#: matched by DIFF_BLOCK_RE first and this never fires on it.
DIFF_BLOCK_UNCLOSED_RE = re.compile(r"```diff[ \t]*\r?\n(.*)", re.DOTALL)


PATCH_SENTINEL_RE = re.compile(r"^\*{3}\s*(Begin|End)\s+Patch\s*$", re.MULTILINE)


def strip_patch_sentinels(diff_text):
    """Drop stray "*** Begin Patch"/"*** End Patch" lines from a unified
    diff. Some models are also trained on OpenAI's "apply_patch" format
    (which wraps a diff-like body in those sentinels) and bleed that
    convention into an otherwise well-formed unified diff -- git apply
    rejects the whole patch outright on the leftover line ("unexpected
    line" / "patch with only garbage"), even though the diff content
    itself is fine.
    """
    return PATCH_SENTINEL_RE.sub("", diff_text).strip() + "\n"


def _diff_header_path(raw):
    """The repo-relative path named by a "--- "/"+++ " header line."""
    path = raw.strip().split("\t")[0].strip()
    if path.startswith(("a/", "b/")):
        path = path[2:]
    return path


def synthesize_git_headers(diff_text):
    """Insert a `diff --git a/X b/X` line before every file block that lacks
    one, so `git apply --recount` can tell where each file's last hunk ends.

    THIS IS NOT COSMETIC -- it is the fix for the fleet's single largest
    patch-rejection cause. Measured on 2026-07-30's 134 rejected diffs:
    86.5% of them patched 2+ files, and in 130 of 130 cases git blamed the
    FIRST file in the diff, never a later one. Single-file diffs applied
    96.4% of the time; multi-file diffs only 24%.

    The mechanism, reproduced in a scratch repo against a diff generated by
    `git diff` itself (i.e. provably correct) with only the `diff --git` and
    `index` lines removed -- exactly the shape models emit:

        --- a/mod.rs                     <- file 1
        +++ b/mod.rs
        @@ -3 +3,2 @@ pub mod b;
         pub mod xmp;
        +pub mod flashpix;
        --- a/sub/other.rs               <- file 2's header
        +++ b/sub/other.rs
        ...

      git apply --recount            -> "error: patch failed: mod.rs:3"
      git apply            (no flag) -> APPLIES
      same diff WITH `diff --git`    -> APPLIES (even with --recount)
      each file split out, --recount -> BOTH APPLY

    --recount deliberately ignores each hunk's stated @@ counts and instead
    reads the hunk body until a line that cannot belong to it. But file 2's
    header line starts with "-", which is a perfectly legal DELETION line, so
    the recounter swallows "--- a/sub/other.rs" (and everything after) into
    file 1's final hunk. That hunk then asks to delete a line the file does
    not contain, and the whole patch is rejected -- and blamed on file 1,
    which is why the failing file was always #1. A `diff --git` line starts
    with "d", cannot be mistaken for hunk content, and terminates the hunk
    cleanly. Every rung of GIT_APPLY_LADDER passes --recount, so no rung
    could ever rescue this; the patch was correct and the applier misread it.

    Replaying all 134 of that day's real rejected diffs against the live
    worker worktrees, this normalization alone recovers 82 (61.2%) with zero
    regressions across 150 sampled previously-applying diffs -- and 61.2% is
    a floor, since those worktrees had moved on by hours.

    Blocks that already carry a `diff --git` line are left untouched, so a
    well-formed git-style diff round-trips byte-identically apart from the
    trailing newline strip_patch_sentinels already applies.
    """
    lines = diff_text.split("\n")
    out = []
    i = 0
    while i < len(lines):
        line = lines[i]
        # A real file block is exactly "--- x", "+++ y", "@@ ...". Requiring
        # the "@@" is what keeps this from firing on hunk BODY text: a
        # deleted line whose content begins "-- " renders as "--- ", and an
        # added line beginning "++ " renders as "+++ ", so the header pair
        # alone is ambiguous (this is the same ambiguity --recount trips
        # over). Body lines are never followed by a hunk header, so the
        # third-line check disambiguates without a stateful parser.
        is_pair = (
            line.startswith("--- ")
            and i + 2 < len(lines)
            and lines[i + 1].startswith("+++ ")
            and lines[i + 2].startswith("@@")
        )
        if is_pair:
            # Walk back over the "index"/mode metadata git puts between the
            # `diff --git` line and the `---` pair, so an already-well-formed
            # block is recognised as such.
            j = len(out) - 1
            while j >= 0 and (
                out[j].startswith(("index ", "old mode ", "new mode ", "new file mode ",
                                   "deleted file mode ", "similarity index ",
                                   "rename from ", "rename to ", "copy from ", "copy to "))
            ):
                j -= 1
            already = j >= 0 and out[j].startswith("diff --git ")
            old_raw = line[4:].strip().split("\t")[0].strip()
            new_raw = lines[i + 1][4:].strip().split("\t")[0].strip()
            # A creation ("--- /dev/null") or deletion ("+++ /dev/null")
            # block is left ALONE. A bare `diff --git a/X b/X` in front of
            # one makes git read it as a modification of an existing X and
            # reject the patch -- git's own output pairs that header with a
            # `new file mode <mode>` / `deleted file mode <mode>` line, and
            # the mode is not recoverable from the diff. Measured: adding
            # the header without the mode line turned 4 previously-applying
            # diffs (a build.rs creation) into failures. Skipping keeps
            # those blocks byte-identical to today's behavior, so this
            # normalization cannot regress them; the cost is only that a
            # block immediately BEFORE a /dev/null block keeps the
            # pre-existing --recount hazard.
            is_devnull = "/dev/null" in (old_raw, new_raw)
            if not already and not is_devnull:
                old = _diff_header_path(old_raw)
                new = _diff_header_path(new_raw)
                if old and new:
                    out.append(f"diff --git a/{old} b/{new}")
        out.append(line)
        i += 1
    return "\n".join(out)


def extract_diff(response_text):
    """Pull a unified diff out of a chat response.

    Prefers a fenced ```diff block; if that fence never closes, falls back
    to everything after the opening fence (see DIFF_BLOCK_UNCLOSED_RE);
    finally falls back to treating the whole response as a diff if it looks
    like one (starts with "diff --git" or "--- "). Returns None if nothing
    diff-shaped is found.

    The unclosed-fence fallback exists for a real, measured failure mode,
    not a hypothetical one: of 361 finalized "no diff in model response"
    replies salvaged from the 2026-08-08/09 fleet run, 8 opened a ```diff
    fence around a real, complete, correctly `--- a/`/`+++ b/`-headed
    unified diff and then never closed it -- instead of a plain closing
    ```, the reply ended on a bare `*** End Patch` line (bled in from the
    unrelated OpenAI apply_patch convention). None of the 8 carried a
    `*** Begin Patch` line or an `*** Update File:` section header -- i.e.
    there is no full apply_patch ENVELOPE anywhere in that corpus to
    translate, only this one sentinel substituting for the fence's closing
    ```. Before this fallback, DIFF_BLOCK_RE's required closing ``` never
    matched, the unfenced fallback below never matched either (the reply
    opens with prose, not "diff --git"/"--- "), and attempt_build treated
    the whole attempt as if no diff had been sent at all -- discarding a
    complete, appliable patch and returning immediately with no repair
    round-trip (contrast the PATCH-chunk and VERIFY paths, which both give
    the model a chance to resend before giving up).

    strip_patch_sentinels (already applied to every branch below) is what
    actually removes the stray `*** End Patch`/`*** Begin Patch` line from
    the captured text, so this fallback only has to worry about WHERE the
    diff content ends -- not what leftover sentinel text needs cleaning
    out of it. Content recovered this way is still passed through the same
    git-apply ladder as any other diff, so a reply that merely LOOKS like
    it opened a diff fence but never wrote real diff content simply fails
    to apply -- exactly as a closed-fence diff that doesn't apply already
    does -- rather than being trusted blindly.

    The result is normalized by synthesize_git_headers so a multi-file diff
    survives --recount; see that function for the measurement behind it.
    """
    match = DIFF_BLOCK_RE.search(response_text)
    if match:
        return synthesize_git_headers(strip_patch_sentinels(match.group(1)))
    match = DIFF_BLOCK_UNCLOSED_RE.search(response_text)
    if match:
        return synthesize_git_headers(strip_patch_sentinels(match.group(1)))
    stripped = response_text.strip()
    if stripped.startswith("diff --git") or stripped.startswith("--- "):
        return synthesize_git_headers(strip_patch_sentinels(stripped))
    return None


def _reply_has_unterminated_diff_fence(reply):
    """True when `reply` opened a ```diff fence but DIFF_BLOCK_RE never
    found a closing ``` -- the signature of a reply that hit config's
    max_tokens reply cap mid-diff, as opposed to one that never attempted
    a diff at all (plain prose, a REQUEST, etc). attempt_build uses this
    to tell the two apart: the first is worth one targeted retry asking
    for PATCH i/N chunking instead of failing the whole attempt outright,
    the second is not."""
    return "```diff" in reply and not DIFF_BLOCK_RE.search(reply)


def estimate_tokens(text):
    """Rough token-count estimate (~4 chars/token, the standard rule of
    thumb for English/code with GPT-style BPE tokenizers). This script
    intentionally has no real tokenizer dependency (it's a uv inline
    script with `dependencies = []`) -- this is a deliberately cheap
    approximation, good enough for staying under a request's token
    budget without pulling in tiktoken or similar just for that."""
    return max(1, len(text) // 4)


#: Section 6: raised from 4096 -- not the critiqued 12288 (TPM economics)
#: -- alongside graduated per-section truncation (assemble_prompt_sections)
#: replacing plain head-keeping, so the extra room actually reaches the
#: learning block instead of just extending how much parser-file text
#: survives.
DEFAULT_MAX_PROMPT_TOKENS = 8192

#: Fallback for config["max_tokens"] (the REPLY cap -- see the module
#: docstring) when a caller doesn't have a config dict to read it from
#: (e.g. the manifest/preview builders). Matches _normalize_model_config's
#: own default so the two can never silently drift apart.
DEFAULT_MAX_TOKENS = 4096


def truncate_to_token_budget(text, max_tokens=DEFAULT_MAX_PROMPT_TOKENS):
    """Hard-truncate text to fit within max_tokens (see estimate_tokens
    for the char/token conversion), keeping the START of the text (the
    framing/instructions/gap description come first in build_prompt's
    output; the large appended reference sections -- parser file
    contents, Perl snippets, memory -- are the least essential once a
    budget this tight is in play) and appending a truncation marker.
    build_prompt's own per-section caps (max_tags, max_file_bytes, etc.)
    already try to keep prompts reasonable; this is the final backstop
    so lowering config's max_prompt_tokens alone is enough to guarantee
    the bound, without having to separately retune every section cap."""
    max_chars = max_tokens * 4
    if len(text) <= max_chars:
        return text
    return (
        text[:max_chars]
        + f"\n\n...(prompt truncated to fit the ~{max_tokens}-token budget; "
        "ask for specific files via REQUEST: if you need something that got cut)"
    )


#: Section 6: the learning block (pitfalls excerpt + module playbook +
#: sweep reviews + lessons tail) gets this many tokens reserved -- always
#: applied as a flat cap (see build_prompt), independent of whatever else
#: is squeezing the rest of the prompt, and never squeezed further than
#: this by assemble_prompt_sections's own overflow pass (it isn't one of
#: the five ranked-priority elastic sections).
DEFAULT_LEARNING_BUDGET_TOKENS = 1200

#: Section 6: the parser-files section never shrinks below this even
#: under the worst overflow -- "elastic with a floor", never squeezed to
#: zero (the inverted-starvation critique this resolves: a huge attempts
#: history must not be able to crowd out the actual source code).
DEFAULT_PARSER_FLOOR_TOKENS = 2000


def _clamp_section_tokens(text, max_tokens):
    """Plain char-truncate (see estimate_tokens's ~4-chars/token rule),
    no marker appended -- used only for the internal graduated section
    shrink in assemble_prompt_sections, which must guarantee a tight
    total-tokens bound; the public truncate_to_token_budget's marker text
    is itself extra tokens on top of its own budget, which would defeat
    that guarantee if used here instead."""
    return text[: max(0, max_tokens) * 4]


def assemble_prompt_sections(sections, budgets, max_tokens):
    """Section 6: graduated per-section truncation, replacing plain
    head-keeping (see truncate_to_token_budget, still used standalone
    elsewhere) so overflow is shed from the LEAST essential sections
    first instead of blindly chopping off everything after some byte
    offset -- which used to delete exactly the learning sections (sweep
    reviews, memory, attempts) that live near the tail.

    sections: ordered [(name, text), ...] covering the ENTIRE prompt, in
    final render order (unlisted-in-budgets sections -- architecture
    constraints, the gap list, Perl NOTES, the learning block, etc --
    are never touched here; their own caps, if any, already applied by
    the caller, e.g. build_prompt's max_tags/learning_budget_tokens).

    budgets: {name: floor_tokens} for the elastic sections eligible to
    be shrunk, in PRIORITY order (dict iteration order = shrink order:
    the first key absorbs overflow first) -- e.g. build_prompt passes
    attempts before samples before neighbor before perl_block before
    parser_files, with parser_files's floor set to parser_floor_tokens
    (never squeezed below that no matter how much overflow remains).

    The two arguments are INDEPENDENT axes and this function keeps them
    that way: render order comes only from `sections`, shrink order only
    from `budgets`. build_prompt relies on that -- since 2026-07-26 its
    render order is chosen to maximise the byte-identical prefix a
    provider's prompt cache can reuse (PROMPT_SECTION_ORDER), which is a
    completely different ranking from what a fixer can afford to lose
    under budget pressure (PROMPT_SHRINK_PRIORITY). Do not "simplify"
    this back into deriving one from the other.

    Algorithm: if the assembled total already fits max_tokens, return it
    unchanged (no section is ever shrunk when there's no pressure to).
    Otherwise walk `budgets` in order; each section not yet at its own
    floor absorbs as much of the remaining overflow as it can without
    going below its floor, and the loop stops the moment overflow reaches
    zero or every listed section is at its floor (remaining overflow, if
    any, is left in the assembled prompt -- floors are a hard guarantee,
    never a target to blow past).

    Returns the assembled prompt string (sections joined in `sections`'
    order, i.e. the exact order given -- callers already handle their
    own inter-section separators inside each section's text)."""
    texts = dict(sections)
    order = [name for name, _ in sections]

    def total_tokens():
        return sum(estimate_tokens(texts[name]) for name in order)

    overflow = total_tokens() - max_tokens
    if overflow <= 0:
        return "".join(texts[name] for name in order)

    for name, floor in budgets.items():
        if overflow <= 0:
            break
        current_text = texts.get(name, "")
        current = estimate_tokens(current_text)
        if current <= floor:
            continue
        shrink_to = max(floor, current - overflow)
        texts[name] = _clamp_section_tokens(current_text, shrink_to)
        overflow -= current - estimate_tokens(texts[name])

    return "".join(texts[name] for name in order)


DEFAULT_COMPACTION_TRIGGER_TOKENS = 12_000
DEFAULT_COMPACTION_KEEP_RECENT_TURNS = 4
DEFAULT_COMPACTION_MIN_ELIDE_TOKENS = 3000
_COMPACTION_STUB_PREFIX = "[earlier content elided for space:"


def compact_messages(messages, trigger_tokens=DEFAULT_COMPACTION_TRIGGER_TOKENS,
                     keep_recent=DEFAULT_COMPACTION_KEEP_RECENT_TURNS,
                     min_elide_tokens=DEFAULT_COMPACTION_MIN_ELIDE_TOKENS):
    """Shrink a long conversation by stubbing out stale served payloads.

    Once the whole conversation's estimated tokens exceed trigger_tokens,
    older USER turns carrying large served content (REQUEST answers,
    VERIFY outputs -- anything over min_elide_tokens) are replaced with a
    one-line stub naming what was elided and how to get it back. Never
    touched: message 0 (the initial prompt), the last keep_recent
    messages, and every assistant message (the model's own diffs/PATCH
    chunks must survive verbatim for chunk reassembly and repair context).
    Pure -- returns a new list; idempotent -- stubs are recognized and
    skipped on a second pass.
    """
    total = sum(estimate_tokens(m["content"]) for m in messages)
    if total <= trigger_tokens:
        return list(messages)
    compacted = list(messages)
    cutoff = max(1, len(compacted) - keep_recent)
    for i in range(1, cutoff):
        msg = compacted[i]
        if msg["role"] != "user":
            continue
        content = msg["content"]
        if content.startswith(_COMPACTION_STUB_PREFIX):
            continue
        if estimate_tokens(content) <= min_elide_tokens:
            continue
        first_line = content.split("\n", 1)[0][:120]
        compacted[i] = {
            "role": "user",
            "content": (
                f"{_COMPACTION_STUB_PREFIX} {first_line} ... "
                "Re-REQUEST it (ideally with a line range) if still needed.]"
            ),
        }
    return compacted


# A 429 is retryable only when it is an rpm rejection. A cost-window cap
# wearing the same status code is NOT -- see classify_429 and the
# RATE_LIMIT_* constants below. 5xx is always retryable.
DEFAULT_RETRYABLE_HTTP_STATUSES = {429, 500, 502, 503, 504}

# What a 429 can mean. They are not interchangeable, and handling them
# identically is what produced 27,662 rate-limit errors over 8 days with no
# way to tell which kind they were.
RATE_LIMIT_RPM = "rpm"                  # going too fast. Retry, with jitter.
RATE_LIMIT_WINDOW_CAP = "window_cap"    # 5h budget spent. Cannot clear for hours.
RATE_LIMIT_TERMINAL_CAP = "terminal_cap"  # weekly budget spent / bad key. Days, or never.
# The gateway says "do not retry" for reasons that are not a spent budget --
# observed live on 2026-08-02: category="internal", code="upstream_rejected",
# retryable=false. Retrying is pointless, but so is parking the endpoint for
# an hour: nothing about the account is exhausted. Fail this call, keep the
# endpoint. Treating this as a cap would idle a healthy worker for an hour
# over one upstream hiccup.
RATE_LIMIT_NON_RETRYABLE = "non_retryable"

# How long this PROCESS parks the offending endpoint after a cap. Local to
# one worker and keyed by base_url: parking Claw Bay must not park a direct
# account, and one worker discovering a spent budget must not pause the
# other N-1 (§0.4 rule 4). A parked endpoint costs one poll per park
# interval, which is cheap enough to sit out a multi-day window unattended.
DEFAULT_WINDOW_CAP_PARK_SECONDS = 300
DEFAULT_TERMINAL_CAP_PARK_SECONDS = 3600
DEFAULT_MAX_RETRIES = 1000
DEFAULT_RETRY_BACKOFF_SECONDS = 2
DEFAULT_MAX_RETRY_BACKOFF_SECONDS = 120  # cap growth -- 2**1000 would otherwise be absurd

# Hard wall-clock ceiling on ONE model call, distinct from `timeout`.
#
# `timeout` is urlopen's socket timeout: it bounds how long a single read
# may block, and every byte that arrives resets it. Under stream=True that
# makes it nearly unbounded in practice -- a provider trickling one SSE
# chunk every few seconds keeps resetting the clock forever. Measured on
# theclawbay.com: a single call ran 2118s against a configured
# timeout=1200, because no individual read ever stalled that long.
#
# deadline_seconds is measured once from the start of the request and
# checked as chunks arrive, so a slow-drip response is abandoned and
# replayed instead of hanging a worker. Kept as a separate knob because
# raising `timeout` is still legitimate for providers that hold a
# connection open before their first token.
DEFAULT_DEADLINE_SECONDS = 120


class ModelCallDeadlineExceeded(Exception):
    """One model call outlived deadline_seconds of wall clock.

    Retryable by design: raising it from inside the `with urlopen(...)`
    block closes the connection (killing the in-flight response), and
    call_model's retry loop then replays the same request. Deliberately
    NOT an HTTPError/URLError subclass -- a slow provider is neither a
    rate limit nor an unreachable host, and conflating it with either
    would report the wrong thing to the rate governor.
    """


class ModelQuotaExhausted(Exception):
    """Raised the moment a 429 is identified as a spent cost budget rather
    than an rpm rejection. No retry is attempted.

    NOT fatal to a worker. attempt_build/review_verdict catch it with the
    rest of the continue-on class (empty 200s, 429s, connection errors);
    it becomes an INFRA_FAILURE_PREFIX reason, which run_tag_loop charges
    nothing for -- no fail increment, no attempt history, no blacklist.

    Policy: a cost cap is TERMINAL for the retry loop. Retrying it cannot
    make budget appear -- the window has to roll over, which takes hours
    (5h) or days (weekly). The old behaviour retried these on the same
    ladder as a momentary rpm rejection and relied on a fleet-wide cooldown
    to pace the resulting hot loop; that cooldown capped at 300s, so a
    weekly cap meant the entire fleet woke, was rejected, and parked again
    every five minutes for days.

    What paces it now is call_model parking the offending ENDPOINT in this
    process (see endpoint_park / DEFAULT_*_CAP_PARK_SECONDS) -- so the
    fleet still resumes unattended the moment the window rolls over, but
    one worker's spent budget no longer pauses any other worker.

    .kind is RATE_LIMIT_WINDOW_CAP or RATE_LIMIT_TERMINAL_CAP, so callers
    and the event stream can distinguish "back in a few hours" from "not
    this week".

    Observed on theclawbay.com, which is explicit about the difference:
        {"error": "weekly cost limit reached for this account",
         "code": "weekly_cost_limit_reached",
         "theclawbayError": {"category": "quota", "retryable": false}}
    """

    def __init__(self, message, kind=RATE_LIMIT_TERMINAL_CAP):
        super().__init__(message)
        self.kind = kind


def classify_429(err):
    """Classify an HTTPError as (kind, message, retry_after) where kind is
    one of the RATE_LIMIT_* constants, message is a short human explanation
    (None for an ordinary rpm rejection) and retry_after is the gateway's own
    `retryAfterSeconds` from the body, or None.

    Reads `theclawbayError` before deciding anything. The real envelope,
    captured live from the gateway on 2026-08-02:

        {"error": "invalid request", "code": "upstream_rejected",
         "theclawbayError": {"requestId": "...", "category": "internal",
                             "code": "upstream_rejected",
                             "userMessage": "...", "retryable": false,
                             "retryAfterSeconds": null, "nextAction": "..."}}

    Two things that envelope settles, both of which contradict a reasonable
    first guess:

    - `retryAfterSeconds` lives in the BODY. A gateway that states how long
      to wait there rather than in a `Retry-After` header would be ignored
      entirely if we only parsed headers, which is precisely the "honour the
      server's own instruction" case that beats any backoff curve.
    - `retryable: false` is NOT synonymous with a spent budget. The gateway
      uses it for ordinary upstream failures too. Treating every
      non-retryable 429 as a cost cap would park a healthy endpoint for an
      hour over one upstream hiccup, so a cap must be QUOTA-shaped --
      `category == "quota"`, or a `*_limit_reached` / cost / quota code.
      Everything else that says "do not retry" becomes
      RATE_LIMIT_NON_RETRYABLE: fail the call, keep the endpoint.

    Conservative by construction and deliberately asymmetric: anything
    unparseable is RATE_LIMIT_RPM, because wrongly retrying a permanent
    condition costs one park interval while wrongly giving up on a transient
    one costs the work.

    A non-429 is never classified here -- it returns (None, None, None).
    """
    if getattr(err, "code", None) != 429:
        return None, None, None
    try:
        # HTTPError's fp is single-read; this is the only consumer, and the
        # body is only needed for classification. Capped so a misbehaving
        # provider can't stream an unbounded "error" at us.
        body = json.loads(err.read(65536))
    except Exception:  # nosec B110 -- unparseable body => treat as retryable
        return RATE_LIMIT_RPM, None, None
    if not isinstance(body, dict):
        return RATE_LIMIT_RPM, None, None
    vendor = body.get("theclawbayError")
    vendor = vendor if isinstance(vendor, dict) else {}
    retryable = vendor.get("retryable", body.get("retryable"))
    code = str(vendor.get("code") or body.get("code") or "")
    category = str(vendor.get("category") or "")
    retry_after = _maybe_positive_float(
        vendor.get("retryAfterSeconds", body.get("retryAfterSeconds"))
    )
    looks_like_quota = (
        category == "quota"
        or code.endswith("_limit_reached")
        or "cost_limit" in code
        or "quota" in code
    )
    # A rejected key is terminal in the strongest sense: every subsequent
    # call to this endpoint fails identically until a human replaces the
    # credential. Retrying is pointless AND parking is right, which is what
    # separates it from the ordinary non-retryable case below.
    looks_like_bad_key = (
        category == "auth" or "api_key" in code or "invalid_key" in code
    )
    if looks_like_bad_key:
        detail = (
            vendor.get("userMessage")
            or body.get("error")
            or code
            or "provider rejected the API key"
        )
        return RATE_LIMIT_TERMINAL_CAP, f"{detail} (code={code or 'unknown'})", retry_after
    if not looks_like_quota:
        if retryable is False:
            detail = (
                vendor.get("userMessage")
                or body.get("error")
                or code
                or "provider reported a non-retryable 429"
            )
            return (RATE_LIMIT_NON_RETRYABLE,
                    f"{detail} (code={code or 'unknown'})", retry_after)
        return RATE_LIMIT_RPM, None, retry_after
    detail = (
        vendor.get("userMessage")
        or body.get("error")
        or code
        or "provider reported a quota 429"
    )
    # A 5h window rolls over on its own within the shift; a weekly one, or
    # a rejected key, does not. Both stop the retry ladder, but they park
    # the endpoint for very different lengths of time.
    lowered = f"{code} {detail}".lower()
    kind = (
        RATE_LIMIT_WINDOW_CAP
        if ("5h" in lowered or "hourly" in lowered)
        else RATE_LIMIT_TERMINAL_CAP
    )
    return kind, f"{detail} (code={code or 'unknown'})", retry_after


def _maybe_positive_float(value):
    """A usable non-negative number, or None. `retryAfterSeconds` is
    explicitly null in the common case, so this must not turn that into 0 --
    a zero delay and an absent delay mean different things."""
    if value is None or isinstance(value, bool):
        return None
    try:
        seconds = float(value)
    except (TypeError, ValueError):
        return None
    return seconds if seconds >= 0 else None


def _quota_exhausted_message(err):
    """Back-compat shim: the message half of classify_429, or None for an
    rpm rejection."""
    return classify_429(err)[1]


CAP_KINDS = (RATE_LIMIT_WINDOW_CAP, RATE_LIMIT_TERMINAL_CAP)


def _retry_after_seconds(headers, now_fn=time.time):
    """Parse a Retry-After header into seconds, or None if absent/unusable.

    RFC 9110 allows either delta-seconds ("30") or an HTTP-date
    ("Wed, 21 Oct 2026 07:28:00 GMT"); providers send both. A server
    telling us exactly how long to wait beats any backoff curve we could
    guess, so this is preferred over the exponential cooldown whenever
    it is larger. Malformed values return None rather than raising --
    a broken header must never take down the loop.
    """
    if not headers:
        return None
    raw = None
    # email.message.Message (urllib's header type) is case-insensitive via
    # .get; a plain dict from a test/mock may not be.
    try:
        raw = headers.get("Retry-After")
    except AttributeError:
        return None
    if raw is None:
        return None
    raw = str(raw).strip()
    if not raw:
        return None
    try:
        return max(0.0, float(int(raw)))
    except ValueError:
        pass
    try:
        when = email.utils.parsedate_to_datetime(raw)
    except (TypeError, ValueError):
        return None
    if when is None:
        return None
    if when.tzinfo is None:
        when = when.replace(tzinfo=datetime.timezone.utc)
    return max(0.0, when.timestamp() - now_fn())


def extract_cache_usage(usage):
    """Pull (cached_input_tokens, total_input_tokens) out of a response's
    `usage` object, tolerating both the OpenAI shape
    (prompt_tokens_details.cached_tokens / prompt_tokens) and the
    Anthropic shape (cache_read_input_tokens / input_tokens).

    Returns None when usage is absent or reports neither -- callers treat
    that as "no cache information available" (log nothing), distinct from
    "(0, N): the provider reported a genuine cache miss". A provider that
    simply doesn't surface cache stats therefore isn't misrecorded as a
    0%-hit-rate call.
    """
    if not isinstance(usage, dict):
        return None
    details = usage.get("prompt_tokens_details")
    if isinstance(details, dict) and "cached_tokens" in details:
        return int(details.get("cached_tokens") or 0), int(usage.get("prompt_tokens") or 0)
    if "cache_read_input_tokens" in usage:
        return int(usage.get("cache_read_input_tokens") or 0), int(usage.get("input_tokens") or 0)
    return None


def apply_prompt_cache_markers(messages, mode):
    """Return a messages list annotated for prompt caching per `mode`,
    without ever mutating the input.

    "explicit": wrap the first user message's content in a single
    Anthropic-style cache_control text block ("ephemeral"), marking the
    large, byte-stable initial prompt (build_prompt orders it static-first
    -- see build_prompt/build_reply_shape_manifest) as one cache
    breakpoint. That first message is identical across a fix_gap's repair
    rounds and largely stable across tags, so it is the single
    highest-value breakpoint. Only providers that accept the block-content
    form plus cache_control benefit; on any other provider this changes
    the request shape, which is exactly why it is opt-in rather than the
    default.

    Anything else ("auto"/"off"): return messages unchanged (plain-string
    content) and rely on the provider's own automatic prefix caching --
    the safe default, since it cannot alter the request into a shape an
    OpenAI-compatible endpoint might reject.
    """
    if mode != "explicit" or not messages:
        return messages
    first = messages[0]
    content = first.get("content")
    if not isinstance(content, str):
        return messages  # already block form (or unexpected) -- leave as-is
    rewritten = dict(first)
    rewritten["content"] = [
        {"type": "text", "text": content, "cache_control": {"type": "ephemeral"}}
    ]
    return [rewritten, *messages[1:]]


def call_model(messages, base_url, api_key, model, max_tokens, reasoning_effort, stream=False, thinking=True,
                temperature=0, timeout=120, max_retries=DEFAULT_MAX_RETRIES,
                retry_backoff_seconds=DEFAULT_RETRY_BACKOFF_SECONDS,
                max_retry_backoff_seconds=DEFAULT_MAX_RETRY_BACKOFF_SECONDS, sleep_fn=time.sleep,
                log_fn=None, usage_fn=None, prompt_cache="auto",
                governor_path=None, governor_calls_per_minute=None, governor_burst=None,
                deadline_seconds=DEFAULT_DEADLINE_SECONDS,
                role=None, event_fn=None, jitter_fn=random.random, now_fn=time.time,
                window_cap_park_seconds=DEFAULT_WINDOW_CAP_PARK_SECONDS,
                terminal_cap_park_seconds=DEFAULT_TERMINAL_CAP_PARK_SECONDS):
    """POST a chat-completions request, retrying on transient upstream
    failures, and return the assistant's reply text.

    Retries (with exponential backoff -- retry_backoff_seconds, *2, *4, ...,
    capped at max_retry_backoff_seconds so a large max_retries doesn't
    imply an absurd wait -- up to max_retries times) on: a retryable
    HTTPError (429/500/502/503/504 -- rate limiting or server-side, not
    this request's fault, confirmed
    to occur in bursts across otherwise-unrelated concurrent workers), a
    connection-level URLError (DNS resolution failure, refused connection,
    TLS handshake failure, or a stalled read -- no HTTP response was ever
    received at all, confirmed live: a DNS outage on the caller's machine
    burned all 10 of one tag's fail-count attempts and got it blacklisted
    without the model ever actually being reachable), or a reply that
    comes back completely empty (a provider occasionally returns "200 OK"
    with zero content -- not a legitimate model answer, indistinguishable
    from a dropped/truncated response, and retrying is cheap compared to
    burning a whole fix attempt on it). A non-retryable HTTPError (4xx
    other than 429: bad request, auth, etc.) fails immediately --
    retrying an actual client-side problem just wastes time and can mask
    a real config issue. max_retries is high (not unlimited) specifically
    to ride out a long transient outage rather than give up and blacklist
    a tag over infrastructure, not the tag itself, being the problem.

    A 429 is NOT one thing. classify_429 reads the provider's
    `theclawbayError` discriminator first:

      - RATE_LIMIT_RPM -- going too fast. Retried on this call's own
        ladder, with full jitter on top of the delay and Retry-After
        honoured when present.
      - RATE_LIMIT_WINDOW_CAP / RATE_LIMIT_TERMINAL_CAP -- the cost budget
        for the 5-hour or weekly window is spent. Retrying cannot make
        budget appear, so it is NOT retried: the endpoint is parked in this
        process (window_cap_park_seconds / terminal_cap_park_seconds) and
        ModelQuotaExhausted is raised immediately.

    Backoff carries FULL jitter (delay + uniform(0, delay), via jitter_fn)
    rather than a bare exponential. This is not rate shaping. N workers
    rejected by the same limit at the same instant compute the same
    exponential delay and retry in the same instant; jitter is what breaks
    that phase-lock. Without it the fleet emits a synchronised burst,
    trips the limit together, and parks together, indefinitely.

    Nothing in this path is shared between workers. One worker's rejection
    never pauses another (§0.4 rule 4) -- the only cross-process state left
    is governor_acquire's steady-state token bucket, which is an rpm budget
    rather than a reaction to failure.

    governor_path (None disables, keeping every old caller byte-identical
    in behavior) points at the cross-process rate-governor state file (see
    governor_acquire): every attempt first acquires one governor slot,
    waiting out the shared token bucket, reusing this call's sleep_fn. The
    governor_* knobs default to None, resolved to the DEFAULT_GOVERNOR_*
    values at call time (they are defined later in this module).

    role (e.g. "fixer", "reviewer") and event_fn, if given, are the
    structured emission the dashboard reads: event_fn is called once per
    attempt outcome with a dict carrying role, model, endpoint, outcome,
    error_class, attempt and latency_s. It is best-effort by contract --
    see _emit -- because telemetry must never be able to fail a model call.

    When stream is True, the response arrives as OpenAI-compatible SSE
    ("data: {...}" lines terminated by "data: [DONE]") -- each chunk's
    choices[0].delta.content is a fragment of the reply. This function
    reassembles those fragments into the same complete string a
    non-streaming call would return, so every caller's contract stays
    identical regardless of which mode is used.

    thinking defaults to True (the API's own default -- omit the field
    entirely rather than guess at an "enabled" shape the docs don't show).
    Set False to send "thinking": {"type": "disabled"}.

    temperature defaults to 0 (deterministic, matching this loop's
    original hardcoded behavior).

    timeout is the socket read timeout in seconds, passed straight to
    urlopen -- some providers hold a streaming connection open with
    keepalives well past 120s before ever sending real content, so this is
    configurable per [worker]/[reviewer] rather than a fixed value.

    deadline_seconds (default DEFAULT_DEADLINE_SECONDS) is the wall-clock
    ceiling on ONE attempt, and is the knob that actually bounds a call.
    timeout alone does not: it limits a single read, and every arriving
    SSE chunk resets it, so a slow-drip stream can run for hours without
    ever tripping it (measured: 2118s against timeout=1200). When the
    deadline is hit the connection is closed and the request is REPLAYED
    like any other transient failure, counting against max_retries. Pass
    None to disable and restore the old unbounded behavior.

    log_fn(str), if given, is called once per retry -- otherwise a worker
    riding out a long stretch of transient failures (a real, intended
    outcome of max_retries being high) produces zero log output for
    however long that takes, which looks indistinguishable from "stuck"
    to anything tailing the log or a dashboard reading it.
    """
    # The DEFAULT_GOVERNOR_* constants live after the Task-1 governor
    # section below this function; resolving them here at call time (not
    # in the def-time defaults) avoids a NameError at import.
    governor_calls_per_minute = (
        DEFAULT_GOVERNOR_CALLS_PER_MINUTE if governor_calls_per_minute is None
        else governor_calls_per_minute
    )
    governor_burst = DEFAULT_GOVERNOR_BURST if governor_burst is None else governor_burst

    def _emit(outcome, error_class=None, started=None, attempt=None, detail=None):
        """Append one structured outcome. Best-effort: a broken sink must
        never fail a model call that otherwise succeeded."""
        if event_fn is None:
            return
        try:
            event_fn({
                "ts": now_fn(),
                "role": role,
                "model": model,
                "endpoint": base_url,
                "outcome": outcome,
                "error_class": error_class,
                "attempt": attempt,
                "latency_s": None if started is None else round(now_fn() - started, 3),
                "detail": detail,
            })
        except Exception:  # nosec B110 -- telemetry is never load-bearing
            pass

    last_error = None
    retry_after = None  # the server's own Retry-After from the previous attempt
    for attempt in range(max_retries + 1):
        if attempt > 0:
            base_delay = min(retry_backoff_seconds * (2 ** (attempt - 1)),
                             max_retry_backoff_seconds)
            if retry_after is not None:
                base_delay = max(base_delay, retry_after)
            # Full jitter on top of the delay: N workers rejected together
            # otherwise compute the same delay and retry together.
            delay = base_delay + base_delay * jitter_fn()
            if log_fn:
                log_fn(
                    f"model call retry {attempt}/{max_retries} after {last_error!r}, "
                    f"waiting {delay:.1f}s"
                )
            sleep_fn(delay)
        retry_after = None
        # This process's own park on this endpoint, from an earlier cap.
        parked = endpoint_park_remaining(base_url, now_fn=now_fn)
        if parked > 0:
            if log_fn:
                log_fn(f"{base_url} is parked for another {parked:.0f}s (cost cap); waiting")
            sleep_fn(parked)
        governor_acquire(governor_path, governor_calls_per_minute, governor_burst,
                         sleep_fn=sleep_fn)
        started = now_fn()
        try:
            reply, usage = _call_model_once(
                messages, base_url, api_key, model, max_tokens, reasoning_effort,
                stream, thinking, temperature, timeout, prompt_cache,
                deadline_seconds=deadline_seconds,
            )
        except ModelCallDeadlineExceeded as e:
            # The provider was reachable and answering, just far too slowly
            # to be worth waiting on -- the connection has already been
            # closed by leaving _call_model_once's `with` block. Replay it.
            # Not a rate limit: a sluggish call must not park an endpoint.
            _emit("error", error_class="deadline", started=started, attempt=attempt)
            last_error = e
            continue
        except urllib.error.HTTPError as e:
            retryable_status = e.code in DEFAULT_RETRYABLE_HTTP_STATUSES
            kind, quota_message, body_retry_after = (
                classify_429(e) if e.code == 429 else (None, None, None)
            )
            if kind == RATE_LIMIT_NON_RETRYABLE:
                # The gateway says not to retry, but nothing is exhausted --
                # so fail this call and leave the endpoint alone. Parking it
                # would idle a healthy worker over one upstream hiccup.
                if log_fn:
                    log_fn(f"429 {kind} ({quota_message}) -- not retrying; endpoint NOT parked")
                _emit("error", error_class=kind, started=started, attempt=attempt,
                      detail=quota_message)
                raise
            if kind in CAP_KINDS:
                # A spent cost budget. Retrying cannot make budget appear,
                # so the ladder stops here -- this is the §2.2(c) fix. The
                # endpoint is parked in THIS process so the worker's next
                # call rides out the window instead of hot-looping, and so
                # that the fleet still resumes unattended when it rolls
                # over. No other worker is affected.
                park = (window_cap_park_seconds if kind == RATE_LIMIT_WINDOW_CAP
                        else terminal_cap_park_seconds)
                endpoint_park(base_url, park, now_fn=now_fn)
                if log_fn:
                    log_fn(
                        f"429 {kind} ({quota_message}) -- not retrying; "
                        f"parking {base_url} for {park}s until the window rolls over"
                    )
                _emit("error", error_class=kind, started=started, attempt=attempt,
                      detail=quota_message)
                raise ModelQuotaExhausted(
                    f"{model} via {base_url}: {quota_message}", kind=kind
                ) from e
            if not retryable_status:
                _emit("error", error_class=f"http_{e.code}", started=started, attempt=attempt)
                raise
            # An rpm rejection or a 5xx. Retry locally. A 429 usually states
            # how long to wait -- in a Retry-After header, or in the
            # gateway's own `retryAfterSeconds` body field (confirmed live
            # 2026-08-02). The server's own statement beats any curve we
            # could guess, so whichever is present raises this worker's next
            # delay, and only this worker's. If both are present, the longer
            # wins: undershooting just earns another 429.
            header_retry_after = (_retry_after_seconds(getattr(e, "headers", None))
                                  if e.code == 429 else None)
            candidates = [v for v in (header_retry_after, body_retry_after) if v is not None]
            retry_after = max(candidates) if candidates else None
            _emit("error", error_class=(RATE_LIMIT_RPM if e.code == 429 else f"http_{e.code}"),
                  started=started, attempt=attempt)
            last_error = e
            continue
        except urllib.error.URLError as e:
            # A connection-level failure (DNS resolution, refused
            # connection, TLS handshake, or a stalled read past timeout)
            # rather than a completed HTTP response -- HTTPError (caught
            # above) is a URLError subclass, so this only matches when no
            # response was ever received at all. Always worth retrying,
            # same as a 5xx: infrastructure being briefly unreachable is
            # not a reason to burn one of this tag's fail-count attempts.
            # Confirmed live: a DNS outage burned all 10 of one tag's
            # attempts and got it blacklisted without the model ever
            # actually being asked -- see urlopen error "nodename nor
            # servname provided" in a real run's attempt history.
            # Connection failures aren't rate limiting: no park.
            _emit("error", error_class="connection", started=started, attempt=attempt)
            last_error = e
            continue
        if not reply:
            _emit("error", error_class="empty_reply", started=started, attempt=attempt)
            last_error = last_error or RuntimeError("model returned an empty reply")
            continue
        if usage_fn is not None:
            usage_fn(usage)
        _emit("ok", started=started, attempt=attempt)
        return reply
    # Only reached once the ENTIRE retry ladder is spent. A cost cap never
    # gets here -- it raises ModelQuotaExhausted on the attempt that saw it.
    # last_error is only None if max_retries < 0 (range(max_retries + 1) never
    # iterates) -- guard against `raise None`, which would raise a confusing
    # TypeError instead of surfacing the actual misconfiguration.
    raise last_error or RuntimeError("call_model: max_retries < 0, no attempt was made")


def _call_model_once(messages, base_url, api_key, model, max_tokens, reasoning_effort, stream, thinking,
                      temperature, timeout, prompt_cache="auto",
                      deadline_seconds=None, now_fn=None):
    """One HTTP attempt. Raises ModelCallDeadlineExceeded if the call
    outlives deadline_seconds of wall clock (None disables the deadline,
    preserving the old unbounded behavior for callers that want it).

    The socket timeout bounds a single stalled read; the deadline bounds
    the whole call. Both are needed: a provider can satisfy the former
    indefinitely by trickling SSE chunks, which is precisely the failure
    this deadline exists to cut off.
    """
    # Resolved here, not as a def-time default: binding time.monotonic at
    # import makes the clock unpatchable, which silently disables every
    # test that injects one -- caught only because a deadline test kept
    # returning the full slow reply instead of raising.
    now_fn = time.monotonic if now_fn is None else now_fn
    started = now_fn()

    def remaining():
        if deadline_seconds is None:
            return None
        return deadline_seconds - (now_fn() - started)

    def check_deadline(where):
        left = remaining()
        if left is not None and left <= 0:
            raise ModelCallDeadlineExceeded(
                f"model call exceeded deadline_seconds={deadline_seconds} while {where}"
            )

    url = base_url.rstrip("/") + "/chat/completions"
    payload = {
        "model": model,
        "messages": apply_prompt_cache_markers(messages, prompt_cache),
        "temperature": temperature,
        "max_tokens": max_tokens,
        "reasoning_effort": reasoning_effort,
        "stream": stream,
    }
    if stream:
        # Without this, OpenAI-compatible providers omit the usage object
        # from a streamed response entirely -- so cache accounting (and
        # token counts) would only ever work for non-streamed calls. The
        # usage arrives in a final choices-empty chunk, captured below.
        payload["stream_options"] = {"include_usage": True}
    if not thinking:
        payload["thinking"] = {"type": "disabled"}
    body = json.dumps(payload).encode()
    req = urllib.request.Request(
        url, data=body, method="POST",
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {api_key}",
            # Some providers (e.g. theclawbay.com) sit behind a Cloudflare
            # WAF that blocks the default "Python-urllib/x.y" User-Agent
            # outright (error code 1010), independent of API key validity.
            "User-Agent": (
                "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) "
                "AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0 Safari/537.36"
            ),
        },
    )
    # Never let one socket read outlast the whole call's deadline: without
    # this, a connection that stalls right before the deadline still blocks
    # for the full (much larger) `timeout` before anything notices.
    effective_timeout = timeout
    left = remaining()
    if left is not None:
        effective_timeout = max(1.0, min(timeout, left))
    # base_url is developer-supplied local config (MODEL_FIX_BASE_URL /
    # REVIEW_BASE_URL), never network- or attacker-controlled input.
    with urllib.request.urlopen(req, timeout=effective_timeout) as resp:  # nosec B310
        if not stream:
            response = json.loads(resp.read())
            check_deadline("reading a non-streamed response")
            return response["choices"][0]["message"]["content"], response.get("usage")

        chunks = []
        usage = None
        for raw_line in resp:
            # Checked per chunk, not per read: this is the only place that
            # can catch a response which is technically still alive but is
            # never going to finish in a useful amount of time. Leaving the
            # `with` block by raising closes the connection, which is what
            # actually kills the in-flight request.
            check_deadline("streaming the response")
            line = raw_line.decode("utf-8").strip()
            if not line.startswith("data:"):
                continue
            data = line[len("data:"):].strip()
            if data == "[DONE]":
                break
            event = json.loads(data)
            # Providers that support it stream a final usage-only chunk
            # (OpenAI's stream_options.include_usage); capture it whenever
            # present so cache accounting works for streamed calls too.
            if event.get("usage"):
                usage = event["usage"]
            choices = event.get("choices") or []
            if not choices:
                continue  # e.g. the final usage-only chunk
            content = (choices[0].get("delta") or {}).get("content")
            if content:
                chunks.append(content)
        return "".join(chunks), usage


#: The git-apply tolerance ladder, tried strictly least-tolerant first; the
#: FIRST rung that applies wins and the rest are never run.
#:
#: Why a ladder at all: a real RW2 worker transcript (deepseek-v4-pro,
#: 2026-07-26T21:23) burned an entire attempt on two consecutive
#: "did not apply" rejections -- a plan+diff at turn 21 and a VERIFY diff at
#: turn 23 -- for a model diff whose content was right but whose context had
#: drifted. There was no second chance: `git apply --recount` either matches
#: 3 lines of context verbatim or fails, and a model that has only seen an
#: excerpt of a file cannot reliably reproduce the surrounding whitespace.
#: Every rung below still requires the change to be LOCATABLE -- there is
#: deliberately no `patch --fuzz` fallback, which can silently apply a hunk
#: in the wrong place and produce a plausible-looking but wrong edit.
#:
#: --recount (every rung) tells git to ignore each hunk's stated
#: @@ -a,b +c,d @@ line counts and recompute them from the actual +/-/context
#: lines instead -- models routinely emit diffs with an off-by-one in that
#: header despite otherwise-correct content, which git rejects outright as
#: "corrupt patch" without this flag. Harmless for a diff whose counts were
#: already right.
#:
#: NOTE the deliberate absence of --reject, which every rung before this
#: ladder used to pass. --reject makes `git apply` NON-ATOMIC: it applies
#: whatever hunks it can, writes the rest to .rej files, and still exits 1.
#: Measured directly (scratch repo, 2026-07-26): a 2-file patch failing on
#: file A left file B fully modified plus an A.rej on disk. That is fatal for
#: a ladder -- rung N+1 would then be matching its context against a tree
#: rung N already half-patched, so a "success" at a looser rung could mean a
#: doubly-applied or interleaved edit. Without --reject, `git apply` checks
#: every hunk before writing anything and leaves the tree byte-identical on
#: failure (also measured), which is exactly the precondition each next rung
#: needs. Nothing in the fleet ever consumed a .rej file: attempt_build calls
#: git_checkout_clean immediately after a failed apply, whose `git clean -fd`
#: both deleted the partially-applied state and printed the "Removing
#: <file>.rej" lines seen in worker logs. That clean still runs and still
#: sweeps .rej files left behind by anything else; this ladder simply stops
#: creating them.
GIT_APPLY_LADDER = (
    # 1. Exact: the pre-ladder behavior (minus --reject), and still the rung
    #    the overwhelming majority of diffs land on.
    ("exact", ["git", "apply", "--recount", "-"]),
    # 2. Indentation slips -- a model re-typing a Rust block from memory gets
    #    the code right and the leading spaces/tabs wrong.
    ("ignore-whitespace", ["git", "apply", "--recount", "--ignore-whitespace", "-"]),
    # 3. Context drift -- the file moved on (another worker's landed fix, or
    #    the model only ever saw an excerpt). -C1 requires 1 line of context
    #    instead of 3; the -/+ lines themselves must still match exactly.
    ("context1", ["git", "apply", "--recount", "-C1", "-"]),
    # 4. Both at once.
    ("context1-ignore-whitespace",
     ["git", "apply", "--recount", "-C1", "--ignore-whitespace", "-"]),
    # 5. Real 3-way merge. Only possible when the diff carries `index
    #    <old>..<new>` blob lines AND that old blob is in this repo's object
    #    store -- most model-authored diffs have neither, so this rung usually
    #    fails instantly with "does not have a valid blob information". Try it
    #    anyway (it costs one exec) but do not rely on it. See
    #    _restore_after_three_way below for the state it can leave behind.
    ("3way", ["git", "apply", "--3way", "--recount", "-"]),
    # 6. Last resort: drop --recount. Every rung above passes it, so a diff
    #    that --recount itself misparses (see synthesize_git_headers: a
    #    multi-file block whose next "--- " header gets swallowed as a
    #    deletion line) fails all five identically. synthesize_git_headers
    #    normally prevents that shape from reaching here at all, but a diff
    #    whose headers could not be reconstructed -- an unparseable path, a
    #    block the model wrote without a "+++" partner -- still can. This
    #    rung requires the model's stated @@ counts to be exactly right,
    #    which is the strictness --recount exists to relax, so it is tried
    #    only after everything else has failed; it can never mask a rung
    #    above it.
    ("no-recount", ["git", "apply", "-"]),
)

#: Rung 5's name, referenced in both the success and failure paths below.
_THREE_WAY_RUNG = "3way"


def _restore_after_three_way(repo_root, applied):
    """Undo the index side-effects `git apply --3way` has and the other
    rungs don't. Measured in a scratch repo, 2026-07-26:

      - On SUCCESS --3way implies --index, so the change comes back STAGED
        ("M  f.txt"). That silently breaks git_checkout_clean: its
        `git checkout -- .` restores the worktree FROM THE INDEX, so a
        3way-applied change would survive the revert that follows a failed
        build and leak into the next round's diff (and into `git add -A`).
      - On FAILURE with a real conflict it exits 1 having written conflict
        markers into the file and left an UNMERGED index entry ("UU f.txt").
        `git checkout -- .` on an unmerged path is an error, and
        git_checkout_clean runs it with check=True -- i.e. that would have
        raised CalledProcessError and killed the worker outright.

    `git reset -q -- <paths>` puts those index entries back to HEAD; on the
    failure path a following `git checkout -- <paths>` then throws away the
    conflict-markered worktree content. Scoped to the paths the 3way itself
    touched (staged or unmerged -- rungs 1-4 stage nothing, so anything
    staged here is ours) rather than the whole tree, so this can never
    discard unrelated work.
    """
    paths = set()
    for argv in (
        ["git", "diff", "--cached", "--name-only"],
        ["git", "diff", "--name-only", "--diff-filter=U"],
    ):
        probe = subprocess.run(argv, capture_output=True, text=True, cwd=repo_root)  # nosec B603
        paths.update(line for line in probe.stdout.splitlines() if line.strip())
    if not paths:
        return
    ordered = sorted(paths)
    subprocess.run(  # nosec B603
        ["git", "reset", "-q", "--"] + ordered, capture_output=True, text=True, cwd=repo_root,
    )
    if not applied:
        subprocess.run(  # nosec B603
            ["git", "checkout", "--"] + ordered, capture_output=True, text=True, cwd=repo_root,
        )


def git_apply_with_rung(diff_text, repo_root):
    """Apply a unified diff to the working tree, walking GIT_APPLY_LADDER
    until one rung succeeds. Returns (success, message, rung_used) --
    rung_used is the GIT_APPLY_LADDER name that applied, or None on total
    failure. git_apply() below is the 2-tuple wrapper every existing caller
    keeps using; only the callers that want to LOG which rung was needed
    (see logging_git_apply) call this one.

    List-argv only, no shell=True anywhere in this file -- repo_root is a
    local path this process already trusts (the repo it's running in), and
    diff_text is passed via stdin, never interpolated into the argv list.
    """
    # Idempotent, and cheap. extract_diff already normalizes what it
    # returns, but attempt_build's chunked "PATCH i/N" path reassembles a
    # diff by concatenating chunk bodies, and test fakes call here directly
    # -- doing it here too means no apply path can miss it.
    diff_text = synthesize_git_headers(diff_text)
    first_error = None
    for rung, argv in GIT_APPLY_LADDER:
        result = subprocess.run(  # nosec B603
            argv, input=diff_text, capture_output=True, text=True, cwd=repo_root,
        )
        applied = result.returncode == 0
        if rung == _THREE_WAY_RUNG:
            _restore_after_three_way(repo_root, applied)
        if applied:
            # "applied" verbatim for the exact rung: that exact string is
            # what the manifest and every existing test have recorded since
            # this function existed, and a looser rung is the only
            # interesting case to call out.
            message = "applied" if rung == GIT_APPLY_LADDER[0][0] else f"applied at rung {rung!r}"
            return True, message, rung
        if first_error is None:
            first_error = result.stderr
    # Report the STRICT rung's stderr, not the last one's: it's the message
    # that names the context git searched for, which is what a model needs to
    # correct its diff. The suffix tells it not to waste its retry on a
    # whitespace/indentation tweak -- that was already tried for it.
    looser = ", ".join(rung for rung, _ in GIT_APPLY_LADDER[1:])
    return False, f"{first_error}(also retried at looser rungs: {looser} -- none applied)", None


def git_apply(diff_text, repo_root):
    """Apply a unified diff to the working tree. Returns (success, message).

    Thin 2-tuple wrapper over git_apply_with_rung so every existing caller
    (attempt_build's git_apply_fn contract, and the test fakes injected for
    it) is unaffected by the ladder's extra return value.
    """
    success, message, _rung = git_apply_with_rung(diff_text, repo_root)
    return success, message


def git_checkout_clean(repo_root):
    """Discard all uncommitted changes, including untracked files."""
    subprocess.run(["git", "checkout", "--", "."], cwd=repo_root, check=True)  # nosec B603
    subprocess.run(["git", "clean", "-fd"], cwd=repo_root, check=True)  # nosec B603


def sanitize_trailer_value(value, max_chars=200):
    """M1: a trailer value must be a single line and bounded -- collapse
    every whitespace run (newlines included) to a single space, then
    hard-truncate. Shared by git_commit's own trailer rendering and any
    caller building trailer values ahead of time (e.g. fix_gap's
    Exiftool-Value/Oxidex-Value from live evidence)."""
    text = " ".join(str(value).split())
    return text if len(text) <= max_chars else text[: max_chars - 1] + "…"


def git_commit(message, repo_root, trailers=None):
    """Commit staged changes with `message`; trailers=None matches every
    existing caller's behavior exactly.

    trailers (spec M1), when given, is an ordered sequence of (key,
    value) pairs -- a plain dict also works for callers with no repeated
    keys, but a list-of-pairs is what lets a cluster commit carry
    multiple `Tag:` trailers, one per member. Each value is sanitized
    (see sanitize_trailer_value) and rendered as one "Key: value" line;
    every trailer line is appended as ONE extra `-m` block (git's own
    "-m block is a paragraph" convention), so `git interpret-trailers
    --parse` -- validate_fix_commit.py's parser, log_sweep_review.py's
    M6 auto-entries -- sees exactly the fleet's evidence-trailer
    contract. A key whose value is None or "" is skipped (an omittable
    trailer like Perl-Ref/Table, per spec M1's "else omit")."""
    subprocess.run(["git", "add", "-A"], cwd=repo_root, check=True)  # nosec B603
    argv = ["git", "commit", "-m", message]
    if trailers:
        items = trailers.items() if isinstance(trailers, dict) else trailers
        lines = [f"{key}: {sanitize_trailer_value(value)}" for key, value in items if value]
        if lines:
            argv += ["-m", "\n".join(lines)]
    subprocess.run(argv, cwd=repo_root, check=True)  # nosec B603


def refresh_worktree(repo_root, base_ref):
    """Fast-forward this worktree's current branch onto base_ref's latest
    commits. Returns (refreshed: bool, message: str).

    Called at the top of every run_tag_loop round (see its
    refresh_worktree_fn) so a worker retrying the same tag across many
    rounds -- --max-tags-per-process=1 means it never picks a different
    tag, only keeps retrying this one until it's fixed or blacklisted --
    doesn't keep comparing against an increasingly stale snapshot of the
    shared branch for however long that takes. Without this, another
    worker can fix and merge the exact same tag while this one is still
    working on it, entirely invisibly: fix_gap's own duplicate-insertion
    check (see detect_duplicate_tag_insertion) is the last line of
    defense for whatever staleness window this doesn't close.

    --ff-only deliberately never attempts a real 3-way merge: this
    worktree should have zero local commits ahead of base_ref at the
    point this runs (a fresh round only starts after the previous
    round's failed attempt was fully reverted, and a successful attempt
    exits the process immediately per --max-tags-per-process=1), so
    the fast-forward should always succeed in practice. If it can't (the
    rare case where that assumption doesn't hold), skip the refresh for
    this round rather than risk a real merge conflict deep inside a
    retry loop -- the next round tries again.

    The fetch is not optional. A remote-tracking ref like origin/main only
    advances when something fetches it, so merging onto it without one
    fast-forwards to whatever this worktree last saw -- which for a
    long-lived worker is the commit it started on. That is a silent no-op
    that looks exactly like a working refresh in the logs: the merge
    succeeds, reports "Already up to date", and the worker goes on
    measuring against a snapshot that is hours stale. A fetch failure is
    NOT fatal here -- the network is allowed to be down -- but it must be
    reported, because a refresh that quietly did nothing is how 606 of 928
    tracked tags ended up already-merged upstream and still being retried.
    """
    fetch = subprocess.run(  # nosec B603
        ["git", "fetch", "origin", "--quiet"],
        cwd=repo_root, capture_output=True, text=True,
    )
    result = subprocess.run(  # nosec B603
        ["git", "merge", "--ff-only", base_ref],
        cwd=repo_root, capture_output=True, text=True,
    )
    message = (result.stdout + result.stderr).strip()
    if result.returncode != 0:
        reconciled, reconcile_msg = _reconcile_published_worktree(repo_root, base_ref)
        if reconciled:
            return True, reconcile_msg
        message = f"{message} [{reconcile_msg}]"
    if fetch.returncode != 0:
        fetch_err = (fetch.stdout + fetch.stderr).strip()
        message = (
            f"{message} [warning: fetch failed, so {base_ref} may be stale: {fetch_err}]"
        ).strip()
    return result.returncode == 0, message


def _reconcile_published_worktree(repo_root, base_ref):
    """Reset to base_ref when this worktree's local commits already landed
    upstream. Returns (reconciled: bool, message: str).

    A worker commits its verified fix and exits (max_tags_per_process=1), so
    on respawn the worktree carries commits base_ref does not -- and
    --ff-only correctly refuses. Left alone it stays diverged forever,
    measuring against the commit it forked from while the rest of the fleet
    moves on. That is the same staleness the fetch fix addressed, arriving by
    a different route.

    Resetting unconditionally would be the obvious fix and the wrong one: it
    would discard a verified fix that has not been published yet. So this
    resets ONLY when every local commit is already reachable upstream by
    patch-id -- i.e. the work landed, possibly squashed under a different sha,
    and the local copy is now redundant. `git cherry` answers exactly that
    question: '-' means an equivalent patch exists in base_ref, '+' means it
    does not.

    If anything is still unpublished ('+'), this deliberately does nothing and
    the worker keeps its commits. Waiting on the tip is recoverable; throwing
    away a verified fix is not.
    """
    cherry = subprocess.run(  # nosec B603
        ["git", "cherry", base_ref, "HEAD"],
        cwd=repo_root, capture_output=True, text=True,
    )
    if cherry.returncode != 0:
        return False, f"cannot compare against {base_ref}: {cherry.stderr.strip()}"

    lines = [line for line in cherry.stdout.splitlines() if line.strip()]
    unpublished = [line for line in lines if line.startswith("+")]
    if unpublished:
        return False, (
            f"holding {len(unpublished)} unpublished commit(s); "
            f"staying put rather than discarding verified work"
        )
    if not lines:
        return False, "diverged but no commits to reconcile"

    reset = subprocess.run(  # nosec B603
        ["git", "reset", "--hard", base_ref],
        cwd=repo_root, capture_output=True, text=True,
    )
    if reset.returncode != 0:
        return False, f"reset to {base_ref} failed: {reset.stderr.strip()}"
    return True, (
        f"reconciled to {base_ref}: {len(lines)} local commit(s) already upstream"
    )


def file_content_at_head(path, repo_root):
    """path's content as of the current branch's HEAD -- i.e. before
    whatever diff is currently applied (uncommitted) to the working
    tree. "" if path doesn't exist there (a brand-new file has nothing
    to have already duplicated)."""
    result = subprocess.run(  # nosec B603
        ["git", "show", f"HEAD:{path}"], cwd=repo_root, capture_output=True, text=True,
    )
    return result.stdout if result.returncode == 0 else ""


DIFF_FILE_HEADER_RE = re.compile(r"^\+\+\+ b/(.+)$", re.MULTILINE)


def detect_duplicate_tag_insertion(diff_text, tag_literal, repo_root):
    """True if diff_text appears to add a REDUNDANT second handler for
    tag_literal (the exact Rust string literal a correct fix inserts,
    e.g. '"APP12:CAM1"') in some file it touches, rather than genuinely
    introducing it for the first time or editing an existing occurrence
    in place.

    Compares tag_literal's occurrence count in each touched file before
    (file_content_at_head) vs after (the file as it sits on disk right
    now, with the diff already applied) the diff: a genuinely new tag
    starts at 0 and ends at 1; an in-place edit of an existing handler
    stays the same (e.g. 1 -> 1); only a redundant duplicate ADDS a new
    occurrence alongside an untouched existing one (1 -> 2). This is
    exactly the shape of every merge conflict this pipeline has hit so
    far: two workers, each unaware of the other, independently wiring up
    a tag that was already fixed and merged while this one was still
    working on it -- a gap refresh_worktree closes for most rounds, but
    not the window between "this round's refresh" and "this diff being
    reviewed", which can still be many minutes on a slow/retried model
    call.
    """
    for path in DIFF_FILE_HEADER_RE.findall(diff_text):
        full_path = Path(repo_root) / path
        try:
            post_text = full_path.read_text()
        except OSError:
            continue
        pre_text = file_content_at_head(path, repo_root)
        pre_count = pre_text.count(tag_literal)
        post_count = post_text.count(tag_literal)
        if pre_count >= 1 and post_count > pre_count:
            return True
    return False


def tag_literal_for_gap(gap):
    """The exact Rust string literal (e.g. '"APP12:CAM1"') a correct fix
    for this single-tag gap should insert -- used by
    detect_duplicate_tag_insertion. None if gap doesn't look like a
    single-tag gap (zero or more than one entry across missing_tags/
    value_differences) -- the duplicate check is skipped rather than
    guessing which of several tags a diff was actually supposed to add.
    """
    entries = gap["missing_tags"] + gap["value_differences"]
    if len(entries) != 1:
        return None
    entry = entries[0]
    if entry.get("tag_key"):
        return f'"{entry["tag_key"]}"'
    family, name = entry.get("family"), entry.get("name")
    if not family or not name:
        return None
    return f'"{family}:{name}"'


def cargo_env():
    """Base env for cargo subprocesses -- opportunistically routes rustc
    through sccache when it's installed, so parallel workers (each its own
    worktree with its own target/ dir) share compiled dependency artifacts
    across worktrees instead of every worker cold-compiling the same ~60
    crates independently. A no-op (falls back to the plain environment,
    i.e. cargo's normal incremental cache only) when sccache isn't on PATH,
    so this never breaks an environment that doesn't have it. Respects an
    explicit RUSTC_WRAPPER already in the environment, and can be disabled
    outright with OXIDEX_USE_SCCACHE=0 (main() sets that env var from
    config.toml's use_sccache knob).
    """
    env = dict(os.environ)
    if (
        os.environ.get("OXIDEX_USE_SCCACHE") != "0"
        and "RUSTC_WRAPPER" not in env
        and shutil.which("sccache")
    ):
        env["RUSTC_WRAPPER"] = "sccache"
    return env


DEFAULT_GOVERNOR_PATH = OXIDEX_HOME / "logs" / "rate-governor.json"
DEFAULT_GOVERNOR_CALLS_PER_MINUTE = 30
DEFAULT_GOVERNOR_BURST = 5


# Per-endpoint park state, PROCESS-LOCAL by construction: a plain dict, not
# a file, not a lock. base_url -> unix time until which this worker will not
# call that endpoint.
#
# This replaces the fleet-wide cooldown that governor_report used to set.
# That cooldown was global, so a single worker's 429 paused every worker;
# against an rpm limit it phase-locked the fleet (all workers released at
# the same instant, synchronised burst, limited together, parked together --
# exactly the shape a rate limiter is built to reject), and against a cost
# cap its 300s ceiling was futile.
_ENDPOINT_PARKED_UNTIL = {}


def endpoint_park(base_url, seconds, now_fn=time.time):
    """Park base_url for `seconds` in THIS process. Extends an existing
    park, never shortens it."""
    until = now_fn() + seconds
    _ENDPOINT_PARKED_UNTIL[base_url] = max(_ENDPOINT_PARKED_UNTIL.get(base_url, 0.0), until)
    return _ENDPOINT_PARKED_UNTIL[base_url]


def endpoint_park_remaining(base_url, now_fn=time.time):
    """Seconds left on this process's park of base_url; 0.0 if free."""
    return max(0.0, _ENDPOINT_PARKED_UNTIL.get(base_url, 0.0) - now_fn())


def endpoint_park_clear(base_url=None):
    """Clear one endpoint's park, or all of them. For tests and for a
    caller that has independent evidence the window rolled over."""
    if base_url is None:
        _ENDPOINT_PARKED_UNTIL.clear()
    else:
        _ENDPOINT_PARKED_UNTIL.pop(base_url, None)


def _governor_locked(path, mutate_fn, now_fn):
    """Run mutate_fn(state) -> (new_state, result) under an exclusive
    flock on path's sibling lockfile, loading/saving the JSON state
    around it. A missing or corrupt state file becomes a fresh
    permissive state -- the governor must never brick the loop over its
    own bookkeeping."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    lock_path = path.with_suffix(".lock")
    with open(lock_path, "w") as lock_f:
        fcntl.flock(lock_f, fcntl.LOCK_EX)
        try:
            state = json.loads(path.read_text())
            if not isinstance(state, dict):
                raise ValueError("state is not a dict")
        except (OSError, ValueError):
            state = {}
        state.setdefault("tokens", float(DEFAULT_GOVERNOR_BURST))
        state.setdefault("last_refill", now_fn())
        # cooldown_until/consecutive_limited are deliberately absent: the
        # fleet-wide cooldown they drove is gone (see _ENDPOINT_PARKED_UNTIL).
        # A state file left over from an older build simply carries two keys
        # nothing reads.
        new_state, result = mutate_fn(state)
        path.write_text(json.dumps(new_state))
        return result


def governor_acquire(path, calls_per_minute=DEFAULT_GOVERNOR_CALLS_PER_MINUTE,
                     burst=DEFAULT_GOVERNOR_BURST, now_fn=time.time,
                     sleep_fn=time.sleep, jitter_fn=random.random):
    """Block until this process may make one model API call.

    Cross-process token bucket, shared by every worker through one
    flock-guarded JSON file: refill at calls_per_minute/60 tokens/sec
    (capped at burst), spend one per call. Waits carry +/-20% jitter so
    workers don't all wake at the same instant. path=None disables (old
    callers, tests).

    This is a steady-state rpm BUDGET, not a reaction to failure. It shapes
    how fast the fleet is allowed to go; it never parks anyone because
    someone else was rejected. The fleet-wide cooldown that used to live
    here -- one worker's 429 pausing every worker on a 30/60/120/240/300
    ladder -- is gone. Rate-limit reactions are per-worker and per-endpoint
    now (see call_model and _ENDPOINT_PARKED_UNTIL).
    """
    if path is None:
        return
    while True:
        def try_take(state):
            now = now_fn()
            rate = calls_per_minute / 60.0
            elapsed = max(0.0, now - state["last_refill"])
            state["tokens"] = min(float(burst), state["tokens"] + elapsed * rate)
            state["last_refill"] = now
            if state["tokens"] < 1.0:
                return state, (1.0 - state["tokens"]) / rate
            state["tokens"] -= 1.0
            return state, None

        wait = _governor_locked(path, try_take, now_fn)
        if wait is None:
            return
        sleep_fn(wait * (0.8 + 0.4 * jitter_fn()))


def cargo_build(repo_root, semaphore_path=None, semaphore_max_holders=DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS):
    """Build the oxidex binary to verify a candidate diff compiles.

    Uses the "fixloop" profile (see Cargo.toml) rather than --release --
    this is a correctness check, not a binary anyone ships, so it isn't
    worth paying release's fat-LTO/single-codegen-unit compile cost on
    every single verification build.

    Returns (success, stderr).

    semaphore_path (spec section 5's build semaphore -- see
    find_tag_gaps.build_semaphore), if given, gates this build behind
    the shared cross-process cargo-build/test slot limit. None (the
    default) keeps this call ungated -- every existing caller/test is
    unaffected unless it opts in.
    """
    with build_semaphore(semaphore_path, semaphore_max_holders):
        result = subprocess.run(  # nosec B603
            ["cargo", "build", "--profile", "fixloop", "--bin", "oxidex"],
            capture_output=True, text=True, cwd=repo_root, env=cargo_env(),
        )
    return result.returncode == 0, result.stderr


def cargo_check(repo_root, semaphore_path=None, semaphore_max_holders=DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS):
    """Fast compile-only check (no codegen, no tests) for VERIFY trial
    diffs -- see attempt_build. Returns (success, output), stdout+stderr
    combined (cargo check's errors go to stderr, but warnings/summaries
    can land on stdout).

    semaphore_path/semaphore_max_holders: see cargo_build."""
    with build_semaphore(semaphore_path, semaphore_max_holders):
        result = subprocess.run(  # nosec B603
            ["cargo", "check", "--workspace"],
            capture_output=True, text=True, cwd=repo_root, env=cargo_env(),
        )
    return result.returncode == 0, result.stdout + result.stderr


def cargo_test_workspace(repo_root, semaphore_path=None, semaphore_max_holders=DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS):
    """Run the full workspace test suite. Returns (success, output) --
    output is stdout+stderr combined (cargo test's failure detail --
    which assertion failed, panic message, etc. -- goes to stdout, not
    stderr, unlike cargo build's compiler errors), so a caller can feed
    the actual failure back to the model instead of just "tests
    regressed" with no detail to act on.

    semaphore_path/semaphore_max_holders: see cargo_build."""
    with build_semaphore(semaphore_path, semaphore_max_holders):
        result = subprocess.run(  # nosec B603
            ["cargo", "test", "--workspace"],
            capture_output=True, text=True, cwd=repo_root, env=cargo_env(),
        )
    return result.returncode == 0, result.stdout + result.stderr


def cargo_test_targeted(repo_root, filter_str, semaphore_path=None,
                         semaphore_max_holders=DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS):
    """Fast first-line test gate: only lib tests whose names match
    filter_str (the format lowercased -- best-effort, zero matches is a
    pass, which cargo already treats as success). The full workspace
    suite still gates every commit; this just stops candidates that are
    about to die at review from paying the full-suite price first.

    semaphore_path/semaphore_max_holders: see cargo_build."""
    with build_semaphore(semaphore_path, semaphore_max_holders):
        result = subprocess.run(  # nosec B603
            ["cargo", "test", "--lib", filter_str],
            capture_output=True, text=True, cwd=repo_root, env=cargo_env(),
        )
    return result.returncode == 0, result.stdout + result.stderr


DEFAULT_MAX_PROMPT_TAGS = 40
DEFAULT_MAX_PROMPT_FILE_BYTES = 60_000


DEFAULT_MAX_SAMPLE_FILES_LISTED = 15


DEFAULT_MAX_ATTEMPT_DIFF_CHARS = 2000

#: How many of a tag's past attempts survive a blacklist-exhaustion reset.
#: The reset exists so a stuck worker can revisit everything it gave up on;
#: it used to do that by DELETING the state entry, which also threw away the
#: attempt history that format_previous_attempts feeds into the next prompt.
#: The tag then repeated the same failures from scratch with no memory of
#: them -- measured on 2026-07-30's live state: 51 tags reached the 10-fail
#: cap having burned 1,998 attempts between them (77.1% of every attempt the
#: fleet has ever recorded), averaging 39.2 attempts each.
#:
#: Keeping the whole history instead would grow the prompt without bound
#: across cycles (format_previous_attempts renders every entry, truncating
#: only each individual diff), so retain a tail: enough for
#: summarize_rejection_codes to tell the next round "all of these failed the
#: same way", bounded at roughly one cycle's worth of distinct approaches.
DEFAULT_RESET_ATTEMPT_HISTORY = 6


#: One-line explanation of each rejection code, shown at the head of the
#: previous-attempts section so a worker does not have to infer what a
#: marker means from the prose around it.
REJECTION_CODE_GUIDANCE = {
    "patch-did-not-apply": (
        "your diff was never written to disk -- a context/path problem, not a logic "
        "problem. The FIX may well have been right."
    ),
    "build-failed": "the diff applied and the compiler rejected it.",
    "tag-still-absent": "it built, and the target tag is still missing from oxidex's output.",
    "wrong-value": (
        "it built and the tag now appears, but its value disagrees with ExifTool -- "
        "chase the value, not the wiring."
    ),
    "gap-set-churned": (
        "it built and DID close a gap; the rebuild revealed another, so the count "
        "stayed flat. The approach works -- extend it."
    ),
    "gap-set-unchanged": (
        "it built and changed NOTHING observable. The code you edited is not running "
        "for this file, or is not the copy that runs. Stop patching it and find out why."
    ),
    "format-unreachable": (
        "oxidex emits nothing format-specific for this format's own sample -- its "
        "parser never executes."
    ),
}


def summarize_rejection_codes(previous_attempts):
    """Roll a tag's attempt history up into "<code> x<n>" counts, most
    frequent first -- the compressed answer to "what keeps going wrong
    here". "" when no attempt carries a rejection code (every attempt
    recorded before this taxonomy existed, and every legacy-contract
    caller), so this section simply does not appear for them."""
    counts = {}
    for attempt in previous_attempts or []:
        code = rejection_code(attempt.get("reason"))
        if code:
            counts[code] = counts.get(code, 0) + 1
    if not counts:
        return ""
    ordered = sorted(counts.items(), key=lambda kv: (-kv[1], kv[0]))
    lines = [
        f"- {code} x{n}: {REJECTION_CODE_GUIDANCE.get(code, '')}".rstrip()
        for code, n in ordered
    ]
    return "How the previous attempts on this tag failed:\n" + "\n".join(lines) + "\n\n"


def clear_blacklist_keeping_history(state, key, keep=DEFAULT_RESET_ATTEMPT_HISTORY):
    """Make `key` claimable again after blacklist exhaustion WITHOUT
    forgetting why it was blacklisted.

    Every suppression field is cleared, so this is exactly as revisitable
    as the `state.pop(key)` it replaces -- including the `unreachable`
    blacklist PR #204 sets for a format whose parser produced no output,
    which is precisely the entry that must come back if a sample file or a
    working parser appears later. What survives is the last `keep`
    attempts: the diffs already tried, why each was rejected, and the
    reviewer critiques. format_previous_attempts renders those into the
    next round's prompt (with summarize_rejection_codes' rollup on top),
    which is the whole point -- popping the entry meant the next cycle
    re-derived the same broken approach from a blank slate.

    A key with no entry stays absent; a key with no history is left with an
    empty list, not a fabricated one.
    """
    entry = state.get(key)
    if entry is None:
        return
    attempts = entry.get("attempts") or []
    kept = list(attempts[-keep:]) if keep > 0 else []
    reset_cycles = int(entry.get("reset_cycles") or 0) + 1
    new_entry = {
        "fails": 0,
        "blacklisted": False,
        "attempts": kept,
        # Observability: distinguishes "never tried" from "tried a full
        # cycle, gave up, and is being retried with what it learned".
        "reset_cycles": reset_cycles,
    }
    # Carry the stable identity/attribution fields; drop every suppression
    # and claim field so the entry is genuinely free to be re-claimed.
    for field in ("tier", "canonical_module", "canonical_table"):
        if field in entry:
            new_entry[field] = entry[field]
    state[key] = new_entry


def format_previous_attempts(previous_attempts, max_diff_chars=DEFAULT_MAX_ATTEMPT_DIFF_CHARS):
    """Render a tag's attempt history (see run_tag_loop's persisted
    per-tag "attempts" list) into a prompt section, so a later round gets
    to see what earlier rounds already tried and why it failed instead of
    repeating the same broken approach from scratch. Each diff is
    truncated -- the point is "what direction was tried", not a byte-exact
    replay -- so this stays bounded even after many rounds' worth of
    history accumulates for one stubborn tag.

    When those attempts carry rejection codes (see annotate_rejection),
    the section opens with summarize_rejection_codes' rollup: the diffs
    below say what was tried, and a worker reading five of them still has
    to work out for itself that all five failed the same way. Absent for
    an unannotated history, so nothing changes for a caller whose
    attempts predate the taxonomy."""
    if not previous_attempts:
        return ""
    blocks = []
    for i, attempt in enumerate(previous_attempts, 1):
        diff = attempt.get("diff")
        if diff:
            shown = diff[:max_diff_chars]
            if len(diff) > max_diff_chars:
                shown += "\n... (truncated)"
            diff_block = f"```diff\n{shown}\n```"
        else:
            diff_block = "(no diff was produced)"
        block = f"Attempt {i}:\n{diff_block}\nFailed because: {attempt.get('reason', 'unknown')}"
        critique = attempt.get("critique")
        if critique:
            block += f"\nReviewer critique: {critique}"
        blocks.append(block)
    return (
        "\n\nPrevious attempts on this exact tag, in order (learn from these -- do not "
        "repeat the same broken approach):\n\n"
        + summarize_rejection_codes(previous_attempts) + "\n\n".join(blocks)
    )


DEFAULT_MAX_PERL_SNIPPETS = 4
DEFAULT_MAX_PERL_SNIPPET_CHARS = 5000
PERL_TAG_ID_RE = re.compile(r"^\s*(0x[0-9A-Fa-f]+|'[^']*'|\"[^\"]*\"|\d+)\s*=>\s*\{")
PERL_BARE_ENTRY_RE = re.compile(r"^\s*(?:0x[0-9A-Fa-f]+|-?\d+)\s*=>\s*['\"][^'\"]*['\"]\s*,?\s*(?:#.*)?$")
PERL_TABLE_HEADER_RE = re.compile(r"^%(Image::ExifTool::\S+)\s*=\s*\(")
PERL_GROUPS_RE = re.compile(r"^\s*GROUPS\s*=>")


@functools.lru_cache(maxsize=1)
def resolve_exiftool_perl_lib_dir():
    """Locate the real ExifTool Perl module directory (Image/ExifTool/*.pm)
    so build_prompt can show the fixer ExifTool's own ground-truth parsing
    logic for a tag -- not just its name and a value it has to
    reverse-engineer a Rust port for from scratch. This is exactly the gap
    behind bugs a human reviewer caught this session that the fixer's own
    build+test pass couldn't: a NEF fix assigned tag 0xA302 the name
    "CFAPattern2" when ExifTool's own source calls it "CFAPattern" (0x828E
    is the real CFAPattern2), because nothing had ever shown the fixer
    ExifTool's actual Exif.pm entries to compare its guess against.

    The pinned source tree comes first, and that ordering is the whole
    point: this directory is the ExifTool Perl the fixer model *reads*
    while writing transcriptions, so it must be the same release the
    transcriptions are graded against. Scraping the PATH exiftool
    resolved to Homebrew's 13.55 Cellar copy while the pin was 13.59 --
    the model was shown one release's tables and scored against
    another's, which is the same skew this module's oracle exists to
    kill, just on the input side.

    The PATH scrape survives as a fallback for a machine with no cached
    checkout. The bare system perl doesn't have Image::ExifTool installed
    as a regular module -- exiftool bundles its own copy and adds it to
    @INC itself -- so this can't just `use Image::ExifTool` and read
    $INC. Homebrew's formula patches the lib path directly into the
    installed script (an `unshift @INC, "<path>/lib/perl5"` line in its
    BEGIN block), so the fallback reads it straight from there rather
    than hardcoding a version-specific Cellar path.
    """
    pinned = exiftool_oracle.pinned_lib() / "Image" / "ExifTool"
    if pinned.is_dir():
        return pinned

    exe = shutil.which("exiftool")
    if not exe:
        return None
    try:
        content = Path(exe).read_text(errors="ignore")
    except OSError:
        return None
    match = re.search(r'unshift @INC, "([^"]+/lib/perl5)"', content)
    if not match:
        return None
    lib_dir = Path(match.group(1)) / "Image" / "ExifTool"
    return lib_dir if lib_dir.is_dir() else None


def _find_perl_tag_block(lines, name_line_idx, context_before_max=15, context_after_max=60):
    """Given the line index of a `Name => 'TagName'` match, find the
    surrounding tag-definition block's start (the enclosing `<id> => {`
    line) and end (the matching `},`/`}` at that same indentation), using
    indentation as a lightweight brace-matcher -- ExifTool's source is
    consistently indented, so this is far simpler than a real Perl parser
    and good enough for "show the fixer the tag's own hash entry". Falls
    back to a small fixed window around the Name line if no `<id> => {`
    opener is found nearby (e.g. a Composite tag defined differently),
    rather than returning nothing.

    Some tags (e.g. EXE.pm's MachO table: `0 => 'CPUArchitecture',`) are
    defined as a bare `<id> => 'Name'` pair with no hash/braces at all --
    checked first and returned immediately, since scanning for a `{`
    opener that doesn't exist would otherwise walk into a neighboring
    tag's block (or fall back to an arbitrary fixed offset) and return
    the wrong tag's source entirely.
    """
    if PERL_BARE_ENTRY_RE.match(lines[name_line_idx]):
        return name_line_idx, name_line_idx

    start_idx = None
    lookback_floor = max(0, name_line_idx - context_before_max)
    for i in range(name_line_idx, lookback_floor - 1, -1):
        if PERL_TAG_ID_RE.match(lines[i]):
            start_idx = i
            break
    if start_idx is None:
        start_idx = max(0, name_line_idx - 2)

    # Some simple tags are defined entirely on one line, e.g.
    # `0x1e => { Name => 'X', Writable => 'int16u' }, #comment` -- if the
    # opener line's braces are already balanced, that line IS the whole
    # block. Scanning forward from it would otherwise walk straight into
    # whatever tag happens to be defined next.
    opener = lines[start_idx].split("#", 1)[0]
    if opener.count("{") > 0 and opener.count("{") <= opener.count("}"):
        return start_idx, start_idx

    indent = len(lines[start_idx]) - len(lines[start_idx].lstrip(" "))
    lookahead_ceiling = min(len(lines), start_idx + context_after_max)
    end_idx = lookahead_ceiling - 1
    for i in range(start_idx + 1, lookahead_ceiling):
        stripped = lines[i].strip()
        line_indent = len(lines[i]) - len(lines[i].lstrip(" "))
        if stripped in ("},", "}") and line_indent == indent:
            end_idx = i
            break
    return start_idx, end_idx


def _find_perl_table_context(lines, block_start_idx, lookback=250):
    """Best-effort table name + GROUPS line for a tag block, found by
    scanning backward from its start for the nearest `%Image::ExifTool::X
    = (` table header, then forward a few lines from there for GROUPS --
    tells the fixer which ExifTool table (and therefore which group/IFD
    naming convention) a tag belongs to, exactly the context that was
    missing when a fixer this session guessed "EXIF:" instead of a
    Panasonic RAW table's real "IFD0" group.
    """
    floor = max(0, block_start_idx - lookback)
    table_name = None
    table_line_idx = None
    for i in range(block_start_idx, floor - 1, -1):
        match = PERL_TABLE_HEADER_RE.match(lines[i])
        if match:
            table_name = match.group(1)
            table_line_idx = i
            break
    if table_name is None:
        return None, None
    groups_line = None
    for i in range(table_line_idx, min(len(lines), table_line_idx + 10)):
        if PERL_GROUPS_RE.match(lines[i]):
            groups_line = lines[i].strip()
            break
    return table_name, groups_line


def extract_perl_tag_snippet(tag_name, lib_dir, tag_id=None, max_chars=DEFAULT_MAX_PERL_SNIPPET_CHARS,
                              format_hint=None):
    """Find tag_name's (or tag_id's, if given) definition in ExifTool's
    real Perl source under lib_dir and return a formatted block showing
    its file, table, GROUPS, and the tag's own hash entry -- or None if
    lib_dir is unavailable or nothing matches (e.g. a derived Composite
    tag with no simple literal definition to find).

    Searches by exact tag ID first when given -- most precise, since it
    distinguishes tags ExifTool happens to give very similar names (e.g.
    CFAPattern vs CFAPattern2) -- falling back to an exact `Name =>
    'tag_name'` match, or a bare `<id> => 'tag_name'` entry (ExifTool's
    shorthand for a tag with no extra attributes -- e.g. EXE.pm's MachO
    table defines `0 => 'CPUArchitecture'` directly, with no `Name =>`
    key at all) otherwise.

    format_hint, if given (typically the gap's format, e.g. "MachO"),
    disambiguates when the SAME tag name is defined in more than one
    table within a single .pm file -- e.g. EXE.pm defines "CPUArchitecture"
    separately for its MachO, PEF, and ELF tables. Without this, a plain
    first-match-wins search picked PEF's entry for a MachO gap purely
    because MachO's own entry uses the bare form the old code didn't
    search for at all, and PEF's came first among what it did find --
    silently showing the fixer the wrong executable format's parsing
    logic. Matches whose enclosing table name contains format_hint
    (case-insensitively) are preferred; only when none do (or no
    format_hint was given) does document order decide, as before.
    """
    if lib_dir is None:
        return None
    id_pattern = None
    if tag_id:
        normalized = tag_id.lower().replace("0x", "").lstrip("0") or "0"
        id_pattern = re.compile(rf"^\s*0x0*{re.escape(normalized)}\s*=>\s*\{{")
    name_pattern = re.compile(r"Name\s*=>\s*['\"]" + re.escape(tag_name) + r"['\"]")
    bare_pattern = re.compile(
        r"^\s*(?:0x[0-9A-Fa-f]+|-?\d+)\s*=>\s*['\"]" + re.escape(tag_name) + r"['\"]\s*,?\s*(?:#.*)?$"
    )

    # Exif.pm is the authoritative source for standard EXIF/TIFF-based tags
    # shared across most formats this loop fixes (JPEG, the RAW formats,
    # TIFF itself), so it's searched first -- a plain alphabetical sweep
    # otherwise risks matching a same-named tag in some unrelated vendor
    # module first (e.g. a generic "Album" tag resolving to Audible.pm's
    # own metadata table instead of the tag actually meant here), which
    # would mislead the fixer worse than showing it nothing at all.
    all_pm_paths = sorted(lib_dir.glob("*.pm"))
    exif_pm = lib_dir / "Exif.pm"
    ordered_paths = ([exif_pm] if exif_pm in all_pm_paths else []) + [
        p for p in all_pm_paths if p != exif_pm
    ]

    for pm_path in ordered_paths:
        try:
            text = pm_path.read_text(errors="ignore")
        except OSError:
            continue
        lines = text.splitlines()
        match_idx = None
        if id_pattern:
            for i, line in enumerate(lines):
                if id_pattern.match(line):
                    match_idx = i
                    break
        if match_idx is None:
            candidates = sorted(
                i for i, line in enumerate(lines)
                if name_pattern.search(line) or bare_pattern.match(line)
            )
            if candidates:
                match_idx = candidates[0]
                if format_hint:
                    for idx in candidates:
                        candidate_start, _ = _find_perl_tag_block(lines, idx)
                        table_name, _ = _find_perl_table_context(lines, candidate_start)
                        if table_name and format_hint.lower() in table_name.lower():
                            match_idx = idx
                            break
        if match_idx is None:
            continue

        start_idx, end_idx = _find_perl_tag_block(lines, match_idx)
        table_name, groups_line = _find_perl_table_context(lines, start_idx)
        snippet = "\n".join(lines[start_idx:end_idx + 1])
        if len(snippet) > max_chars:
            snippet = snippet[:max_chars] + "\n... (truncated)"
        header = pm_path.name
        if table_name:
            header += f", table {table_name}"
        header_block = f"--- {header} ---"
        if groups_line:
            header_block += f"\n{groups_line}"
        return f"{header_block}\n```perl\n{snippet}\n```"
    return None


def build_perl_reference_block(gap, lib_dir, max_tags_shown=DEFAULT_MAX_PERL_SNIPPETS):
    """Collect ExifTool's real Perl source for as many of this gap's tags
    as can be found, capped the same way missing_tags/parser_files are --
    the point is grounding the fixer in ExifTool's actual parsing logic
    for the tags actually in front of it this round, not an exhaustive
    reference dump."""
    if lib_dir is None:
        return ""
    seen = set()
    candidates = []
    for t in gap["missing_tags"]:
        name = t.get("name")
        if name and name not in seen:
            seen.add(name)
            candidates.append((name, t.get("tag_id")))
    for d in gap["value_differences"]:
        name = d["tag_key"].split(":")[-1]
        if name not in seen:
            seen.add(name)
            candidates.append((name, None))

    blocks = []
    for name, tag_id in candidates[:max_tags_shown]:
        snippet = extract_perl_tag_snippet(name, lib_dir, tag_id=tag_id, format_hint=gap.get("format"))
        if snippet:
            blocks.append(snippet)
    if not blocks:
        return ""
    return (
        "\n\nExifTool's own Perl source for these tags (ground truth for how ExifTool "
        "actually parses/formats them -- port the logic, not a guess at it; if the Rust "
        "port needs to differ, know exactly what you're diverging from and why):\n\n"
        + "\n\n".join(blocks)
    )


DEFAULT_MAX_PRECEDENT_CHARS = 3000
SIBLING_LITERAL_RE_TEMPLATE = r'"({family}:[A-Za-z0-9_]+)"'


def find_implemented_sibling(gap, repo_root):
    """First already-implemented same-family tag literal in the gap's own
    parser files, excluding the gap's own tags -- the nearest working
    example of 'how tags in this table get wired'."""
    families, own = set(), set()
    for e in gap["missing_tags"]:
        families.add(e["family"]); own.add(f'{e["family"]}:{e["name"]}')
    for d in gap["value_differences"]:
        fam = d["tag_key"].split(":")[0]
        families.add(fam); own.add(d["tag_key"])
    for f in gap["parser_files"]:
        try:
            text = (Path(repo_root) / f).read_text(errors="replace")
        except OSError:
            continue
        for family in sorted(families):
            for m in re.finditer(SIBLING_LITERAL_RE_TEMPLATE.format(family=re.escape(family)), text):
                if m.group(1) not in own:
                    return m.group(1)
    return None


def _run_git(args, cwd):
    try:
        result = subprocess.run(  # nosec B603 -- fixed git argv, no untrusted input
            ["git", *args], capture_output=True, text=True, cwd=cwd, timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    return result.stdout if result.returncode == 0 else ""


def build_neighbor_precedent_block(gap, repo_root, git_runner_fn=None):
    """The historical diff that added the nearest implemented sibling tag
    -- nearly every landed fix this loop has produced was 'generalize the
    neighboring tag's branch' (LightS from ColorMode, MODE3-6 from
    MODE1/2, ZipCRC from ZipBitFlag), so showing how the neighbor was
    added (implementation AND its test) up front replaces a whole
    REQUEST investigation. Empty string on any miss; never fatal."""
    git_runner_fn = git_runner_fn or _run_git
    sibling = find_implemented_sibling(gap, repo_root)
    if not sibling:
        return ""
    files = list(gap["parser_files"])
    sha = git_runner_fn(["log", "-S", sibling, "-1", "--format=%H", "--", *files], repo_root).strip()
    if not sha:
        return ""
    patch = git_runner_fn(["show", sha], repo_root)
    if not patch:
        return ""
    if len(patch) > DEFAULT_MAX_PRECEDENT_CHARS:
        patch = patch[:DEFAULT_MAX_PRECEDENT_CHARS] + "\n... (truncated)"
    return (
        f"\n\nHow the neighboring tag {sibling} was added to this same code path "
        "(historical commit -- pattern-match this, including adding an equivalent test):\n"
        f"{patch}"
    )


# ---------------------------------------------------------------------------
# T3 TABLE-PORT: full %table source extraction (spec S3)
# ---------------------------------------------------------------------------
#
# extract_perl_tag_snippet (above) shows ONE tag's own hash entry embedded
# inside a table -- the right amount of context for a single-tag fix.
# A table PORT needs the fixer to see the table's COMPLETE membership at
# once, in ExifTool's own declaration order, so it can port the whole
# thing in one pass rather than reverse-engineering membership tag by
# tag. extract_perl_table_source below is that: the full source span of
# one `%Image::ExifTool::<table_name> = ( ... );` declaration.

DEFAULT_MAX_TABLE_SOURCE_CHARS = 12_000


def _perl_nesting_delta(line):
    """Net paren/brace nesting change contributed by one ExifTool Perl
    source line, with quoted strings and trailing comments crudely
    stripped first so a `PrintConv => 'sprintf("%d (#%x)")'` line doesn't
    corrupt the count. Line-based and heuristic by design -- ExifTool's
    source is consistently formatted, and (like attribute_gaps.py's own
    near-identical _nesting_delta, which this deliberately duplicates
    rather than imports -- attribute_gaps.py is a standalone CLI script,
    not a library this module should take a dependency on for one small
    helper) a few-percent noise rate here is an accepted tradeoff, not a
    correctness requirement: the table-port acceptance gate
    (evaluate_table_port_gate) is what actually verifies correctness,
    this only has to find "roughly the right span of source text to show
    the fixer"."""
    stripped = re.sub(r"'(?:[^'\\]|\\.)*'", "''", line)
    stripped = re.sub(r'"(?:[^"\\]|\\.)*"', '""', stripped)
    hash_idx = stripped.find("#")
    if hash_idx != -1:
        stripped = stripped[:hash_idx]
    return stripped.count("(") + stripped.count("{") - stripped.count(")") - stripped.count("}")


def extract_perl_table_source(table_name, lib_dir, max_chars=DEFAULT_MAX_TABLE_SOURCE_CHARS):
    """The COMPLETE source of one ExifTool `%table` declaration -- spec
    S3: "the full Perl table source (not the per-tag snippet
    extract_perl_tag_snippet gives today)".

    table_name is the table's name as it appears after
    "Image::ExifTool::" in the source, e.g. "Canon::CameraSettings" or
    "XMP::exif" -- NOT including the leading "%Image::ExifTool::" or a
    trailing " = (". Finds the `%Image::ExifTool::<table_name> = (`
    header line, then walks forward counting paren/brace nesting
    (_perl_nesting_delta) until it returns to zero -- tolerant of a
    table spanning many lines and nested hashes/arrays, unlike a fixed
    lookahead window.

    The natural first-guess file (table_name's own leading module
    segment + ".pm", e.g. "Canon::CameraSettings" -> Canon.pm) is tried
    first; every other *.pm/*.pl under lib_dir is tried as a fallback
    (some tables are declared in a shared file -- Exif.pm alone holds
    several IFD tables under other modules' logical namespaces).

    Returns None if lib_dir is unavailable or no file defines this
    table; the caller (attempt_table_port) treats that as "show no
    table-source section" rather than failing the job outright, exactly
    like extract_perl_tag_snippet's own None contract.
    """
    if lib_dir is None:
        return None
    lib_dir = Path(lib_dir)
    header_re = re.compile(r"^%Image::ExifTool::" + re.escape(table_name) + r"\s*=\s*\(")
    module_guess = table_name.split("::", 1)[0] + ".pm"
    guess_path = lib_dir / module_guess
    all_paths = sorted(list(lib_dir.glob("*.pm")) + list(lib_dir.glob("*.pl")))
    ordered_paths = ([guess_path] if guess_path in all_paths else []) + [
        p for p in all_paths if p != guess_path
    ]

    for pm_path in ordered_paths:
        try:
            lines = pm_path.read_text(errors="ignore").splitlines()
        except OSError:
            continue
        start_idx = next((i for i, line in enumerate(lines) if header_re.match(line)), None)
        if start_idx is None:
            continue

        depth = _perl_nesting_delta(lines[start_idx])
        end_idx = len(lines) - 1
        for i in range(start_idx + 1, len(lines)):
            depth += _perl_nesting_delta(lines[i])
            if depth <= 0:
                end_idx = i
                break

        snippet = "\n".join(lines[start_idx:end_idx + 1])
        truncated = len(snippet) > max_chars
        if truncated:
            snippet = snippet[:max_chars] + "\n... (truncated)"
        header = f"--- {pm_path.name}, table Image::ExifTool::{table_name} ---"
        return f"{header}\n```perl\n{snippet}\n```"
    return None


# oxidex-tags-* holds the GENERATED tag name/id database (build.rs
# codegen), not per-module Rust source easily read as text from Python --
# resolving its build-time output would mean either running `cargo build`
# just to inspect it, or reverse-engineering build.rs's own codegen
# inputs. The MakerNotes registries under
# src/parsers/tiff/makernotes/registries/<module>.rs (spec S3's own
# example: canon.rs's CAMERA_SETTINGS_SCHEMA) are checked-in, already
# human-curated Rust source -- readable directly as text, with each
# array-index entry naming its id and tag name side by side
# (`ArrayIndexDef::with_i16_decoder(1, "MacroMode", &MACRO_MODE)`,
# `ArrayIndexDef::raw(2, "SelfTimer")`), which is exactly the "id -> name
# skeleton" shape spec S3 wants -- so this reads THAT as the acceptable
# equivalent the spec explicitly allows ("if reading the generated YAML
# directly is impractical in this scope, reading the equivalent generated
# Rust registry source ... is an acceptable equivalent -- pick whichever
# is actually readable from Python and document the choice"). Documented
# choice: Rust registry source, not the generated YAML.
REGISTRIES_RELATIVE_DIR = Path("src") / "parsers" / "tiff" / "makernotes" / "registries"

_RUST_SCHEMA_VAR_RE = re.compile(r"^\s*static\s+(\w+)\s*:\s*ArraySchema\s*=\s*ArraySchema\s*\{")


def build_table_port_registry_skeleton(module, table_name, repo_root,
                                        registries_dir=REGISTRIES_RELATIVE_DIR,
                                        max_chars=DEFAULT_MAX_TABLE_SOURCE_CHARS):
    """Spec S3 (ii): the oxidex-side id->name SKELETON for a table port,
    labelled unambiguously as scaffolding only -- structure, never value
    ground truth (that's what the Perl source from
    extract_perl_table_source is for).

    module is the ExifTool module name (attribute_gaps.py's "module" key,
    e.g. "Canon") -- looked up as <registries_dir>/<module.lower()>.rs.
    table_name (ExifTool's table name, e.g. "CameraSettings" -- the part
    after the last "::") is matched, case-insensitively and
    punctuation-stripped, against each `static <NAME>_SCHEMA: ArraySchema`
    block's own variable name or `name: "..."` field, so
    "CameraSettings" matches CAMERA_SETTINGS_SCHEMA's `name:
    "CameraSettings"` line.

    Returns None (never fatal) if repo_root/registries_dir/module.rs
    doesn't exist, or no schema block in it matches table_name -- the
    caller shows no skeleton section rather than failing the job, same
    contract as every other build_prompt-style optional block in this
    module.
    """
    if not module:
        return None
    short_table = table_name.split("::")[-1] if table_name else ""
    normalized_target = re.sub(r"[^a-z0-9]", "", short_table.lower())
    if not normalized_target:
        return None

    rs_path = Path(repo_root) / registries_dir / f"{module.lower()}.rs"
    try:
        lines = rs_path.read_text(errors="ignore").splitlines()
    except OSError:
        return None

    for i, line in enumerate(lines):
        match = _RUST_SCHEMA_VAR_RE.match(line)
        if not match:
            continue
        var_name = match.group(1)
        # Scan forward for this block's own `name: "..."` field and its
        # closing `};` (top-level struct literal -- braces at this
        # block's own nesting only, tracked the same way
        # _perl_nesting_delta tracks Perl's).
        depth = _perl_nesting_delta(line)
        block_lines = [line]
        name_field = None
        end_idx = i
        for j in range(i + 1, len(lines)):
            block_lines.append(lines[j])
            name_field_match = re.search(r'name\s*:\s*"([^"]+)"', lines[j])
            if name_field_match and name_field is None:
                name_field = name_field_match.group(1)
            depth += _perl_nesting_delta(lines[j])
            if depth <= 0:
                end_idx = j
                break
        normalized_var = re.sub(r"[^a-z0-9]", "", var_name.lower())
        normalized_name_field = re.sub(r"[^a-z0-9]", "", (name_field or "").lower())
        if normalized_target in (normalized_var, normalized_name_field):
            snippet = "\n".join(lines[i:end_idx + 1])
            truncated = len(snippet) > max_chars
            if truncated:
                snippet = snippet[:max_chars] + "\n... (truncated)"
            return (
                f"--- {registries_dir / f'{module.lower()}.rs'}, {var_name} "
                "(SCAFFOLDING ONLY -- structure/id-to-name skeleton, NOT value ground truth; "
                "the Perl source above is the ground truth) ---\n"
                f"```rust\n{snippet}\n```"
            )
    return None


PERL_MODULE_HEADER_RE = re.compile(r"^--- (\S+\.pm)(?:, table (\S+))?", re.MULTILINE)
NOTES_START_RE = re.compile(r"^\s*NOTES\s*=>\s*q[qw]?([{(\[])\s*$")
_NOTES_CLOSERS = {"{": "}", "(": ")", "[": "]"}
DEFAULT_MAX_NOTES_CHARS = 1500


def _extract_notes_from_lines(lines, max_chars):
    """Find the first `NOTES => q{...}` (or q(...)/q[...]) block within
    the given lines and return its body text, or None if there isn't one
    (or it's malformed/unterminated)."""
    for i, line in enumerate(lines):
        match = NOTES_START_RE.match(line)
        if not match:
            continue
        closer = _NOTES_CLOSERS[match.group(1)]
        body_lines = []
        for candidate_line in lines[i + 1:]:
            if re.match(rf"^\s*{re.escape(closer)},?\s*$", candidate_line):
                text = "\n".join(body_lines).strip()
                if not text:
                    return None
                if len(text) > max_chars:
                    text = text[:max_chars] + "..."
                return text
            body_lines.append(candidate_line)
        return None  # unterminated -- malformed or this scan's assumption doesn't hold; give up cleanly
    return None


def extract_perl_table_notes(pm_path, max_chars=DEFAULT_MAX_NOTES_CHARS, table_name=None):
    """Find ExifTool's own prose description of a table/format -- written
    by the person who actually implemented support for it -- from a
    `NOTES => q{...}` block in a Perl module. Returns None if there's no
    such block (many tables don't have one).

    Some modules (e.g. EXE.pm) hold several unrelated tables side by
    side, each with its own NOTES -- MachO, PEF, and ELF executable
    formats are all defined in the same file. Without table_name this
    just returns the FIRST NOTES block in the file, which for a
    multi-table module is often a different format's documentation
    entirely (e.g. showing Windows PE's NOTES for a MachO gap). When
    table_name is given (see build_format_overview_block, which reads it
    straight off build_perl_reference_block's own "--- X.pm, table Y ---"
    header), this instead scopes the search to that table's own span --
    from its `%Table::Name = (` header line up to the next table header
    or end of file -- and returns None (not a wrong-table fallback) if
    that specific table has no NOTES of its own.

    Uses the opening delimiter's matching closer at start-of-line
    indentation as the end marker -- good enough for ExifTool's
    consistently-formatted NOTES blocks (verified against APP12.pm,
    Exif.pm, etc.), not a full Perl quote-like-operator parser, which
    would need to handle arbitrary nesting/escaping this text never
    actually uses in practice.
    """
    try:
        lines = pm_path.read_text(errors="ignore").splitlines()
    except OSError:
        return None

    if table_name is None:
        return _extract_notes_from_lines(lines, max_chars)

    table_header_re = re.compile(r"^%" + re.escape(table_name) + r"\s*=\s*\(")
    table_start = None
    for i, line in enumerate(lines):
        if table_header_re.match(line):
            table_start = i
            break
    if table_start is None:
        return None

    table_end = len(lines)
    for i in range(table_start + 1, len(lines)):
        if PERL_TABLE_HEADER_RE.match(lines[i]):
            table_end = i
            break

    return _extract_notes_from_lines(lines[table_start:table_end], max_chars)


ARCHITECTURE_PRIMER = """
How oxidex is structured, for orientation (see the actual parser file(s) below for this format's real code):
- Format-specific parsers live under src/parsers/<format>/ (e.g. src/parsers/jpeg/, src/parsers/raw/ for RAW formats sharing a TIFF-based structure, src/parsers/macho/, src/parsers/elf/). Each inserts tags into a MetadataMap as "Group:TagName" -> TagValue pairs.
- Group prefixes should come from this codebase's own naming convention for that code path -- either lookup_tag_name()'s IFD-based lookup (src/tag_db/mod.rs) for standard TIFF/EXIF-style tags, or whatever literal prefix neighboring tags in the same file already use for a format with its own ad hoc segment structure (e.g. JPEG APP12's "APP12:" prefix). Match the existing pattern in the file you're editing; don't introduce a new one.
- TagValue has variants for common types (String, Integer, Rational, Binary, etc.) -- check src/core/ for the exact enum if the type isn't obvious from neighboring code.
- oxidex-tags-* crates (oxidex-tags-core, oxidex-tags-camera, etc.) hold the generated tag name/ID database itself, not per-format parsing logic -- that's what lookup_tag_name() queries.
""".strip()


def build_perl_notes_block(lib_dir, perl_reference_block):
    """ExifTool's own NOTES documentation for this gap's relevant Perl
    module(s) -- extracted from whichever files build_perl_reference_block
    already found tags in, so no redundant module discovery. "" when
    nothing is found.

    Split out of build_format_overview_block (2026-07-26) purely for
    prompt-cache reasons: the primer half of that block is byte-identical
    for every worker/format/tag forever, while this half tracks whichever
    Perl module the CURRENT tag lives in and so varies within a format.
    Measured over 216 renders of real gaps across 22 formats: the primer
    had 1 distinct value across all of them, these NOTES had 19 (and
    varied tag-to-tag within 6 of the 22 formats). Keeping them in one
    section forced the invariant primer to sit behind a per-tag string,
    which ends the cacheable prefix early -- build_prompt now emits them
    in different stability tiers. build_format_overview_block below keeps
    its original combined contract for every other caller."""
    notes_blocks = []
    if lib_dir is not None:
        module_table_pairs = sorted(set(PERL_MODULE_HEADER_RE.findall(perl_reference_block)))
        for name, table_name in module_table_pairs:
            notes = extract_perl_table_notes(lib_dir / name, table_name=table_name or None)
            if notes:
                label = f"{name}, table {table_name}" if table_name else name
                notes_blocks.append(f"--- {label} ---\n{notes}")

    if not notes_blocks:
        return ""
    return (
        "\n\nExifTool's own documentation for this format (from the Perl source's NOTES):\n\n"
        + "\n\n".join(notes_blocks)
    )


def build_format_overview_block(lib_dir, perl_reference_block):
    """Combine ExifTool's own NOTES documentation for this gap's relevant
    Perl module(s) (see build_perl_notes_block) with a short,
    always-included primer on how oxidex's own parsers are organized. The
    per-tag Perl snippets and the full parser file contents (see
    build_prompt's other sections) already show the specifics; this
    section is deliberately just orientation."""
    return f"\n\n{ARCHITECTURE_PRIMER}{build_perl_notes_block(lib_dir, perl_reference_block)}"


RUST_ARCHITECTURE_CONSTRAINTS = """
CRITICAL RUST ARCHITECTURE CONSTRAINTS (you are porting ExifTool's Perl to Rust -- do NOT write "Perl in Rust"):
1. STATE: No new global mutable state (no `static mut`, no interior-mutability statics). Thread endianness/base-offset context as explicit function parameters through the function signatures, or the file's existing endian-aware reader -- exactly like neighboring functions do (ExifTool's own Perl mutates a global byte order; never mirror that).
2. TYPES: No dynamic-typing crutches -- no Box<dyn Any>, no serde_json::Value, no new ad hoc HashMap<String, X> mimicking Perl's autovivified hashes. Use this codebase's strictly-typed TagValue enum into MetadataMap (src/core/), which exist for exactly this.
3. BYTES: Parse binary by slicing &[u8] through the existing FileReader/reader helpers (or nom/winnow where the surrounding file already uses them) -- never the regex crate on bytes, and never refactor a whole parser onto new lifetimes for a one-tag fix.
4. TREES: No self-referential structs for IFD/directory trees. Store absolute byte offsets (usize) or indices, matching ExifTool's own offset-based traversal.
5. BLOAT: Never inline a massive lookup table into a diff. Wire names/IDs through the existing tag database (oxidex-tags-*, lookup_tag_name()); if a huge dictionary is genuinely required, stub it `// TODO: codegen dictionary` and implement only the parsing logic.
6. ERRORS: No unwrap()/expect()/panic!() on data derived from the parsed file -- propagate Result<T, ExifToolError> (src/error/) so one malformed tag can't kill the parse.
7. PERL MAP: unpack("N",...) -> u32::from_be_bytes, unpack("V",...) -> u32::from_le_bytes, unpack("n"/"v") -> u16::from_be_bytes/u16::from_le_bytes, substr($v, off, len) -> a bounds-checked slice &v[off..off + len].
""".strip()


KNOWN_PITFALLS = """
Lessons from mistakes a human reviewer previously caught in this loop's own output (avoid repeating these):
- Never hardcode a group prefix like "EXIF:" on a tag name. Use this codebase's existing lookup_tag_name()/tag_db (or whatever the surrounding code in the file you're editing already uses) so the prefix matches the IFD/table the tag was actually parsed from, consistent with every neighboring tag -- a hardcoded prefix that diverges from the file's own convention has been wrong every time.
- Before writing a new decoder, grep the codebase for the tag's name (e.g. `rg CFAPattern` or `rg TagName`). A correct, already-tested implementation may exist elsewhere (e.g. under src/core/) that just isn't wired into the code path you're fixing -- reuse or match it, don't reinvent it with different (possibly conflicting) logic.
- Two ExifTool tags can have very similar names for genuinely different tag IDs (e.g. CFAPattern at 0xA302 vs CFAPattern2 at 0x828E). Always match by the exact tag ID shown in ExifTool's source, never by name similarity or memory of what a tag "should" be called.
- Verify a tag's display format against ExifTool's actual default text output (what `exiftool file` prints), not `-j`/JSON output -- JSON array/bracket syntax (e.g. [1,2]) is JSON's own serialization, not ExifTool's plain-text tag-value convention (which is usually comma-space-separated, e.g. "1, 2").
""".strip()


def build_reply_shape_manifest(max_prompt_tokens, max_reply_tokens=DEFAULT_MAX_TOKENS):
    """The complete reply protocol, stated once near the top of the
    prompt (stable text -> provider prompt-cache friendly; early text ->
    survives truncate_to_token_budget, which keeps the head).

    max_prompt_tokens is the built-PROMPT budget (config["max_prompt_tokens"],
    default 8192) -- unused by the text below, kept as this function's
    first/positional arg for call-site compatibility. max_reply_tokens is
    the actual constraint shape 3 (PATCH i/N) is about: config["max_tokens"],
    the cap on the model's own REPLY (default 4096, usually about half of
    max_prompt_tokens). Quoting max_prompt_tokens there instead -- as this
    function used to, unconditionally -- told the model a diff was "safely
    under budget" at roughly double the size that would actually get it
    truncated mid-diff, which is exactly why models weren't electing PATCH
    chunking before hitting the real cap."""
    return f"""You are operating in an ephemeral, isolated git worktree; broken builds during investigation are expected and cost nothing -- probe aggressively with VERIFY rather than guessing.

STRATEGY: REQUEST is free -- reading real files costs you nothing and there is no limit on how many you read, so never submit a diff written from a guess about code you could simply have looked at. What you should not do is read AIMLESSLY: prefer the read that resolves a specific uncertainty over another one "just to be thorough". Alongside that, bias toward putting a real candidate in front of the compiler early: if the parser file(s) shown below already give you enough to sketch a plausible fix, send a VERIFY of your best-guess diff rather than continuing to investigate. Use the `cargo check` feedback from each VERIFY to correct course, and expect to iterate 2-3 VERIFY rounds (wrong field offset, wrong PrintConv string, missing import, etc.) before your final Plan + diff -- that loop is cheap and expected. REQUEST and VERIFY are complementary, not competing: REQUEST answers "what does this code/byte layout actually say", VERIFY answers "does my change compile and typecheck". Reach for whichever your current uncertainty calls for.

Every reply must be exactly one of these four shapes:

1. REQUEST: <path> -- see a source file or a sample file (a bare line, nothing else in the reply). Add a 1-indexed line range after a source path -- prefer one for anything large. All four shapes work: `:40-120` (that range), `:400-` (line 400 to end of file), `:-120` (start through line 120), `:400` (a window around line 400). These are UNLIMITED and free: read as many files as you need. Only a REQUEST that buys you nothing is charged against a small allowance -- a path that does not resolve, a range starting past the end of a file, or re-asking for content still visible in this conversation -- and an answer tells you only when it charged you. So check the directory listing an unresolved path comes back with instead of guessing again, and re-read your own earlier answers instead of re-requesting them (if an earlier answer was elided to save space, re-REQUESTing it is free). Distinct line ranges of the same file are distinct reads and stay free when they show new lines; a range that resolves to exactly what you were already shown counts as a re-ask.
2. VERIFY -- trial-compile a candidate change without committing to it: the line "VERIFY" followed by exactly ONE ```diff fenced block. The diff is applied, `cargo check` runs, the tail of its output comes back, and the change is REVERTED -- your final diff must still contain the complete change.
3. PATCH 1/N -- your ENTIRE reply, including this plan text and the diff, is capped at roughly {max_reply_tokens} tokens (~{max_reply_tokens * 4} characters); a reply that runs longer gets cut off mid-diff and the whole attempt is wasted. If your finished diff would come anywhere close to that on its own, don't risk it -- split it into N consecutive chunks and send the first as the line "PATCH 1/N" followed by ONE ```diff fenced chunk; you'll be prompted for each next chunk. Chunks are concatenated in order before applying, so split anywhere (mid-hunk is fine) -- never repeat or skip lines across a boundary.
4. Plan + diff -- first, 2-3 sentences: which tag(s) you're fixing, where in the code, what you learned from the previous turn's output, and (on a retry) what you're doing differently from the failed attempt(s) above and why. Then exactly ONE ```diff fenced block containing the complete unified diff.

Shapes 1-3 are control signals: the control line must be the VERY FIRST line of the reply, with no narrative before it."""


TERMINAL_REMINDER = (
    "Reply now with exactly one of the four shapes defined at the top: "
    "REQUEST, VERIFY, PATCH i/N, or plan + a single ```diff block."
)


# --- K2/K3: shared knowledge layer (replaces per-worker format-memory) ------
#
# Spec K1 retires append_format_memory_note/summarize_format_memory/
# build_format_memory_summary_prompt entirely: a worker's learning now
# becomes a lesson event (see append_lesson/the fix_gap writers below)
# instead of a private note appended to a file the distiller elsewhere
# rewrites (that was the distiller-vs-appender lost-update race). The
# distiller (scripts/distill_lessons.py) is the ONLY writer of both files
# read below; every build_prompt call here is read-only.

def load_global_pitfalls(home=OXIDEX_HOME):
    """K2: fresh read of <home>/logs/knowledge/GLOBAL-PITFALLS.md at every
    build_prompt call (see build_prompt's knowledge_home docstring for the
    hermetic-by-default None-omits gate). Curated only by the distiller or
    a human, always via tempfile+os.replace (see
    distill_lessons.update_global_pitfalls) -- reading it here is fine even
    mid-write, since a reader only ever sees a complete old or complete new
    file. Missing/unreadable/empty falls back to the KNOWN_PITFALLS
    constant, so a fresh rollout (or a hermetic test pointed at an empty
    tempdir) behaves exactly like the pre-K2 hardcoded-constant loop."""
    path = Path(home) / "logs" / "knowledge" / "GLOBAL-PITFALLS.md"
    try:
        text = path.read_text().strip()
    except OSError:
        return KNOWN_PITFALLS
    return text or KNOWN_PITFALLS


def load_module_playbook(knowledge_home, module_key):
    """K3: <knowledge_home>/logs/knowledge/modules/<module_key>.md --
    written ONLY by scripts/distill_lessons.py (workers only ever read, at
    build_prompt time, replacing load_format_memory). "" when missing/
    unreadable/no knowledge_home/no module_key, same "section omitted"
    contract as every other optional build_prompt source."""
    if not knowledge_home or not module_key:
        return ""
    path = Path(knowledge_home) / "logs" / "knowledge" / "modules" / f"{module_key}.md"
    try:
        return path.read_text().strip()
    except OSError:
        return ""


# K4: 4 per tier (same-format + up to 4 cross-format rejections) -- "all 29
# verdicts fit in a prompt" per the spec's own sizing rationale.
DEFAULT_MAX_SWEEP_REVIEW_ENTRIES = 4


#: K4 verdict spellings (legacy binary + K4 verdict_class) that count as a
#: rejection -- the only class allowed to generalize across formats.
_REJECTED_VERDICTS = {"rejected", "human_rejected", "machine_rejected"}
_HUMAN_VERDICT_CLASSES = {"human_accepted", "human_rejected"}


def _is_rejection_entry(entry):
    return (entry.get("verdict") in _REJECTED_VERDICTS
            or entry.get("verdict_class") in _REJECTED_VERDICTS)


def _entry_is_human(entry):
    """K4: 'prompt selection always prefers human entries over machine
    ones'. A pre-K4 entry (no verdict_class at all) was always hand-typed
    by a human reviewer, so it counts as human here too."""
    verdict_class = entry.get("verdict_class")
    return not verdict_class or verdict_class in _HUMAN_VERDICT_CLASSES


def _dedupe_machine_entries(entries):
    """Drop a machine entry sharing (patch_id, reason) with one already
    kept (spec K4: 'deduped ... so a re-polled failure cannot flood the
    window and evict human verdicts'). Human entries are never deduped by
    identity -- distinct human judgment calls are never redundant."""
    seen = set()
    out = []
    for entry in entries:
        if not _entry_is_human(entry) and entry.get("patch_id") and entry.get("reason"):
            key = (entry["patch_id"], entry["reason"])
            if key in seen:
                continue
            seen.add(key)
        out.append(entry)
    return out


def _select_tier(entries, cap):
    """Human-preferred, newest-first selection within one K4 tier.
    `entries` must already be newest-first. Every human entry is kept
    ahead of machine ones (up to `cap` total), then the combined pick is
    restored to newest-first order (picking humans first can otherwise
    put an older human entry ahead of a newer one that got bumped)."""
    if not entries:
        return []
    deduped = _dedupe_machine_entries(entries)
    human = [e for e in deduped if _entry_is_human(e)]
    machine = [e for e in deduped if not _entry_is_human(e)]
    selected = human[:cap]
    if len(selected) < cap:
        selected += machine[: cap - len(selected)]
    order = {id(e): i for i, e in enumerate(entries)}
    selected.sort(key=lambda e: order.get(id(e), len(entries)))
    return selected


def load_recent_sweep_reviews(log_path, format_name,
                              max_entries=DEFAULT_MAX_SWEEP_REVIEW_ENTRIES,
                              max_other_format_entries=DEFAULT_MAX_SWEEP_REVIEW_ENTRIES):
    """Read scripts/log_sweep_review.py's JSONL log and return recent
    entries relevant to format_name, newest first.

    Spec K4 two-tier selection: up to `max_entries` (default 4) most
    recent SAME-format entries (any verdict -- accepted teaches a
    convention worked, rejected teaches it didn't), PLUS up to
    `max_other_format_entries` (default 4) most recent REJECTIONS from
    every OTHER format ("rejections generalize" -- a PrintConv-byte-check
    or duplicate-emission lesson learned on Canon.pm applies to Nikon.pm
    too). Within each tier, human-verdict entries are always preferred
    over machine ones (see _select_tier), and machine entries are
    deduped by (patch_id, reason) so a re-polled merger/sweep failure
    cannot flood the window and evict human verdicts. Cross-format
    entries carry a synthetic "_sweep_review_tier": "other_format" key
    (harmless extra dict key -- format_sweep_review_history uses it to
    render them under their own subheader; nothing else reads it).

    Unlike format_previous_attempts (which only ever sees a build/test
    failure on the *exact same tag* being retried), this surfaces actual
    sweep-review verdicts across many tags, so a fixer working on a
    *different* tag still benefits from what a reviewer already found.

    Missing/corrupt log: returns [] -- this is advisory context, never a
    hard dependency (matches load_tag_state's own "missing file = nothing
    recorded yet" handling).
    """
    if not log_path.exists():
        return []
    same_format, other_format = [], []
    try:
        with log_path.open() as f:
            for line in f:
                line = line.strip()
                if not line:
                    continue
                try:
                    entry = json.loads(line)
                except json.JSONDecodeError:
                    continue
                if entry.get("format") == format_name:
                    same_format.append(entry)
                elif _is_rejection_entry(entry):
                    other_format.append(dict(entry, _sweep_review_tier="other_format"))
    except OSError:
        return []
    # file is append-order (oldest first); newest first for display/selection
    same_format.reverse()
    other_format.reverse()
    return (_select_tier(same_format, max_entries)
            + _select_tier(other_format, max_other_format_entries))


def format_sweep_review_history(entries):
    """Render load_recent_sweep_reviews' output into a prompt section.

    Spec K4: same-format entries render under the original heading;
    cross-format rejections (tagged "_sweep_review_tier": "other_format")
    render under their own subheader, each line naming its source format
    so "the mistakes generalize" reads clearly even though the tag/format
    differs from the one currently being fixed."""
    if not entries:
        return ""

    def render_lines(items, show_format):
        lines = []
        for entry in items:
            verdict = entry.get("verdict", "unknown").upper()
            tag = entry.get("tag", "?")
            reason = entry.get("reason", "no reason given")
            prefix = f"{entry.get('format', '?')}:" if show_format else ""
            lines.append(f"  - {verdict} {prefix}{tag}: {reason}")
        return lines

    same = [e for e in entries if e.get("_sweep_review_tier") != "other_format"]
    other = [e for e in entries if e.get("_sweep_review_tier") == "other_format"]

    sections = []
    if same:
        sections.append(
            "\n\nRecent sweep-review outcomes for this format (a human reviewer's actual "
            "verdicts on other fixes in this format -- REJECTED means a diff that built and "
            "tested fine was still wrong; learn the specific reason, not just that it failed):\n"
            + "\n".join(render_lines(same, show_format=False))
        )
    if other:
        sections.append(
            "\n\nRejections from other formats (the mistakes generalize):\n"
            + "\n".join(render_lines(other, show_format=True))
        )
    return "".join(sections)


DEFAULT_INLINE_SAMPLE_MAX_BYTES = 4096


def build_exact_sample_block(gap, samples_dir):
    """For a gap targeting exactly one tag (run_tag_loop's per-tag mode),
    if that tag's real ExifTool source_file is known (see
    ExifToolExtractor::parse_single_file_json in Rust, which now reads
    ExifTool's own "SourceFile" JSON field), give the model the actual
    sample data for THIS tag specifically -- not just a generic
    per-format sample list it has to guess among.

    Small enough to fit comfortably in the prompt: inline the full hex
    dump directly, no round-trip needed. Too large: clearly name the
    exact file and its size and point at the REQUEST: protocol, rather
    than leaving it to be found (or missed) among samples_block's
    generic per-format list.

    The lead-in says "the tag targeted below", not "this exact tag":
    build_prompt renders this section ABOVE the gap list (2026-07-26),
    because different tags of one format very often come from the SAME
    sample file -- measured over 216 renders of real gaps, this block had
    only 1.86 distinct values per format against the gap list's 4.73, so
    putting the hex dump in front of the gap list keeps a mean 1.2 KB of
    prefix cacheable that the old order re-billed every call (81.6% ->
    87.6% of the prompt cacheable in the offline harness). The wording
    has to point forwards for that to read correctly.
    """
    all_entries = gap["missing_tags"] + gap["value_differences"]
    if len(all_entries) != 1:
        return ""
    source_file = all_entries[0].get("source_file")
    if not source_file:
        return ""
    path = Path(source_file)
    try:
        size = path.stat().st_size
    except OSError:
        return ""
    shown_path = path
    if samples_dir is not None:
        try:
            shown_path = path.relative_to(samples_dir)
        except ValueError:
            pass  # not under samples_dir -- shown_path keeps the full absolute path set above
    if size <= DEFAULT_INLINE_SAMPLE_MAX_BYTES:
        data = path.read_bytes()
        return (
            f"\n\nReal sample file containing the tag targeted below ({shown_path}, {size} bytes) "
            f"-- full hex dump:\n{hex_dump(data, max_bytes=DEFAULT_INLINE_SAMPLE_MAX_BYTES)}"
        )
    return (
        f"\n\nReal sample file containing the tag targeted below: {shown_path} ({size} bytes, too "
        f"large to inline here). Respond with \"REQUEST: {shown_path}\" instead of a diff if "
        "you need to see its raw bytes."
    )


# --- Section 6: adaptive response-format guidance --------------------------
#
# "no diff in model response" is attempt_build's verdict when
# extract_diff() finds neither a ```diff fenced block nor a reply that
# starts like a raw unified diff. It accounts for 1489 of 4922 live
# lesson rows (2026-07-25) -- ~40% of every failure the fleet records,
# nearly 4x the next reason -- and it is a RESPONSE-FORMAT failure, not a
# domain-knowledge one: the model usually did the work and then wrapped
# it in ```rust, or narrated the patch in prose, or emitted a bare
# @@ hunk. No amount of ExifTool lore in the prompt fixes that; a
# corrective statement of the expected envelope does.
#
# It is injected adaptively -- only for the workers actually failing this
# way -- because the fix is worth ~120 tokens and the learning budget is
# 1200: charging every prompt for it permanently would cost 10% of the
# block to teach the ~60% of workers who already get the format right
# something they already know.
#
# Declared here, ahead of the lessons tail, because the tail DEFERS to
# it: select_module_lessons drops these rows from the domain-lessons
# ranking entirely (see there), so each failure class is routed to the
# one section that can actually fix it.

DIFF_FORMAT_FAILURE_RE = re.compile(r"(?i)no diff in model response")

#: One occurrence in the tail window is enough to escalate. This failure
#: mode is self-reinforcing (the model that answers in prose once answers
#: in prose again on the retry) and the corrective text is cheap, so
#: waiting for a second sample buys nothing but another wasted round.
DEFAULT_DIFF_FORMAT_ESCALATION_THRESHOLD = 1


# --- Section 6: lessons tail (part of the learning block) ------------------

DEFAULT_LESSONS_TAIL_KB = 256
DEFAULT_LESSONS_TAIL_MAX_ENTRIES = 8

#: One rendered lessons/quarantine line is clamped to this many chars.
#: Reasons carry whole cherry-pick transcripts and cargo output; a single
#: un-clamped one can be 2000 bytes (the K1 line cap) and eat the entire
#: learning budget by itself.
LESSON_REASON_DISPLAY_CHARS = 240


def _clamp_reason(reason, limit=LESSON_REASON_DISPLAY_CHARS):
    """Whitespace-flatten and clamp one ledger reason to a single prompt
    line. Flattening matters as much as clamping: a cherry-pick or cargo
    reason is multi-line, and an embedded newline would silently split one
    "  - " bullet into several unindented lines mid-block."""
    text = " ".join(str(reason or "").split())
    return text if len(text) <= limit else text[: limit - 1] + "…"


def read_lessons_tail_events(lessons_path, tail_kb=DEFAULT_LESSONS_TAIL_KB):
    """Section 6: every parseable, non-infra K1 row in the last `tail_kb`
    KB of <home>/logs/lessons.jsonl, in ledger order (oldest first), via
    seek -- bounded, no full scan of a ledger that only ever grows.

    A byte offset can land mid-line (either the seek itself, or a writer
    mid-append at the moment of the read) -- the first split chunk after
    a nonzero offset is dropped rather than risking a truncated JSON
    object; every other malformed line is skipped the same way every
    other K1 reader skips one (never degrades to {}). Missing file (not
    yet created, or no lessons_path given): [].

    This is the SINGLE choke point where infra noise leaves the worker's
    view: a row is dropped when its event is "infra" OR when its reason
    reads as a provider outage however it was labelled (see
    distill_lessons.is_infra_reason -- 231 live rows label an outage as
    build_failed/review_rejected/structural). Everything downstream --
    the module/format ranking, the per-worker diff-format detector -- is
    built on this list, so none of them can leak one.
    """
    if not lessons_path:
        return []
    path = Path(lessons_path)
    try:
        size = path.stat().st_size
    except OSError:
        return []
    offset = max(0, size - tail_kb * 1024)
    try:
        with path.open("rb") as f:
            f.seek(offset)
            data = f.read()
    except OSError:
        return []
    lines = data.split(b"\n")
    if offset > 0:
        lines = lines[1:]  # drop a possibly-partial leading fragment
    events = []
    for raw in lines:
        raw = raw.strip()
        if not raw:
            continue
        try:
            ev = json.loads(raw)
        except (UnicodeDecodeError, ValueError):
            continue
        if not isinstance(ev, dict):
            continue
        event = ev.get("event")
        if not event or event == "infra" or is_infra_reason(ev.get("reason")):
            continue
        events.append(ev)
    return events


def select_module_lessons(events, module_key, format_name,
                          max_entries=DEFAULT_LESSONS_TAIL_MAX_ENTRIES):
    """Scope read_lessons_tail_events' rows to this module (when
    module_key is given) else this format, then rank them by RECURRENCE
    -- see distill_lessons.rank_by_recurrence. Returns
    [(representative_row, occurrence_count), ...], most-repeated first.

    Ranking replaced a plain newest-first tail: the tail spent its whole
    budget on whatever the last hour happened to emit, which on a fleet
    where one failure mode accounts for ~40% of all rows means N nearly
    identical lines and no room for the mistake this module actually
    keeps repeating. Collapsing by fingerprint costs one line per
    distinct mistake instead of one per occurrence, so the same budget
    now buys `max_entries` DISTINCT lessons rather than `max_entries`
    samples.

    Two row classes are excluded outright before ranking.

    (1) "no diff in model response" rows. Ranking by recurrence without
    this would be self-defeating: measured over the live tail 2026-07-25
    that one reason is 40 of 235 rows and duly ranks FIRST for
    essentially every module, evicting the wrong_value and structural
    lessons that are the only ones carrying ExifTool knowledge. It is a
    response-format defect, it teaches a worker nothing about the module,
    and it is already routed to the section that can actually fix it
    (build_diff_format_remediation) -- so it does not also get to spend a
    slot here.

    (2) `critique` rows, for two independent reasons that happen to have
    the same fix.

    First, they are DUPLICATES BY CONSTRUCTION. fix_gap writes two rows
    per failed round, not one: the specific event (build_failed /
    wrong_value / gap_not_closed / structural / test_regressed) and then
    critique_and_continue's lesson("critique", critique). The critique row
    is the critic's PARAPHRASE of the very failure the row beside it
    already states, so ranking both double-counts one event. This is the
    same duplicate-row shape already fixed for infra rows in fix_gap
    (which stopped writing a build_failed row beside every `infra` row).
    Excluding (1) by literal reason match could never catch the paraphrase
    -- "The fixer likely emitted no diff because...", "provided a prose
    description instead of an actual unified diff" contain no literal
    "no diff in model response" -- so the diff-format failure went on
    taking slots through its critique twin: 14 of 43 ranked slots across
    the 6 live formats, 2026-07-25 (MRW 5 of 6, NEF 4 of 6).

    Second, a critique reason is unbounded LLM prose, and recurrence
    ranking cannot cluster prose. fingerprint_scoped's reason component is
    normalized free text, so two critiques of the same mistake share no
    key. Measured per event over the same 235-row tail: critique 112 rows
    -> 107 clusters, 106 of them singletons (95%); build_failed 78 rows ->
    2 clusters, 0 singletons; gap_not_closed 11 -> 1; review_rejected 10
    -> 2. Every other event clusters; the critique event alone does not,
    and a ranking whose entries are all singletons is just newest-first
    with extra steps -- precisely the ranking this function replaced.

    Nothing is lost by dropping them: the critique is fed back IN FULL and
    unclamped to the worker that earned it, in the same conversation
    (critique_and_continue appends it to `messages`). What was being
    ranked here was a 240-char truncation of someone else's critique of
    someone else's round.
    """
    if module_key:
        scoped = [ev for ev in events if ev.get("module") == module_key]
    else:
        scoped = [ev for ev in events if ev.get("format") == format_name]
    scoped = [ev for ev in scoped
              if ev.get("event") != "critique"
              and not DIFF_FORMAT_FAILURE_RE.search(str(ev.get("reason") or ""))]
    return rank_by_recurrence(scoped, max_entries=max_entries)


def format_lessons_tail(ranked):
    """Render select_module_lessons' [(row, count), ...] into a
    learning-block section. The "xN" recurrence count is shown only when
    N > 1: on a one-off it is noise, and on a repeat it is the whole
    point -- it tells the model this is a rake the fleet keeps stepping
    on, not one worker's bad afternoon."""
    if not ranked:
        return ""
    lines = []
    for ev, count in ranked:
        event = ev.get("event", "?")
        reason = _clamp_reason(ev.get("reason"))
        tag_key = ev.get("tag_key") or ""
        suffix = f" ({tag_key})" if tag_key else ""
        recurrence = f" x{count}" if count > 1 else ""
        lines.append(f"  - {event}{recurrence}: {reason}{suffix}")
    return (
        "\n\nRecent lessons ledger entries (fleet-wide, spec K1 -- other "
        "workers' outcomes on this module/format, ranked by how often each "
        "distinct mistake RECURS, most-repeated first; \"xN\" is the "
        "recurrence count):\n"
        + "\n".join(lines)
    )


# --- Section 6: the worker's own quarantine verdicts -----------------------
#
# scripts/squad_merge_loop.py's mergers validate every worker commit and
# cherry-pick it onto squad/<name>; a rejected commit is appended to
# <home>/logs/quarantine.jsonl (see squad_merge_loop.append_quarantine for
# the entry shape) and NEVER retried. Until now nothing told the worker:
# model_fix_loop.py contained zero references to that ledger, so a worker
# whose commit was rejected went on stacking new commits on top of the
# rejected code -- which is how 7 heads came to hit cherry-pick conflicts
# for no reason other than their OWN earlier commit having been silently
# dropped from the squad branch. Surfacing the verdict is the whole fix:
# the flags name the defect ("printconv-mismatch:<value>",
# "cherry-pick-conflict", "targeted-test-failed"), so a worker that sees
# them can stop reproducing it.

#: The quarantine ledger is tiny next to lessons.jsonl (77 lines vs 4922
#: live, 2026-07-25) but it is append-only and grows, so it gets the same
#: bounded-seek treatment rather than a full read.
DEFAULT_QUARANTINE_TAIL_KB = 64

#: At most this many of the worker's own rejections reach the prompt.
#: Four is the same window K4 gives sweep reviews: enough to show a
#: pattern, small enough that a bad afternoon cannot crowd out the
#: module playbook.
DEFAULT_QUARANTINE_MAX_ENTRIES = 4

#: A cherry-pick reason embeds git's entire conflict transcript (~700
#: chars live). The flags carry the actual verdict, so the reason is
#: clamped harder than a lesson's.
QUARANTINE_REASON_DISPLAY_CHARS = 180

#: The rendered flags list of ONE entry is capped at this many flags and
#: this many chars.
QUARANTINE_MAX_FLAGS = 4
QUARANTINE_FLAGS_DISPLAY_CHARS = 200

#: squad_merge_loop writes this literal when a candidate commit has no
#: `Format:` trailer to read (squad_merge_loop.py:1111,
#: `commit_format_trailer(...) or "UNKNOWN"`). It is a placeholder for a
#: MISSING format, not a format, and must be read back as one -- see
#: _quarantine_entry_format.
QUARANTINE_UNKNOWN_FORMAT = "UNKNOWN"


def _quarantine_entry_format(entry):
    """The format a quarantine entry actually pins itself to, or None for
    a squad-wide verdict that pins itself to no format at all.

    Two shapes mean "no format": JSON null (overlord_sweep's bisection
    quarantines a whole squad with format_name=None) and the literal
    "UNKNOWN" that squad_merge_loop substitutes when the commit carries no
    Format: trailer. Only the first was recognised, and since "UNKNOWN" is
    truthy the second was silently pinned to a format no worker ever has:
    28 of 77 live rows (36%, exactly 2 per squad across all 14 squads,
    2026-07-25) were invisible to every worker -- and ALL 28 carry
    `missing-trailer:Format` as their first flag, so the single rejection
    class whose defect IS the absent trailer was the one class this
    feature could never report back. The worker therefore kept omitting
    the trailer, and the ledger kept growing two more rows per squad.
    """
    fmt = str(entry.get("format") or "").strip()
    if not fmt or fmt == QUARANTINE_UNKNOWN_FORMAT:
        return None
    return fmt


def read_own_quarantine(quarantine_path, worker_label, format_name,
                        tail_kb=DEFAULT_QUARANTINE_TAIL_KB,
                        max_entries=DEFAULT_QUARANTINE_MAX_ENTRIES):
    """This worker's OWN recent quarantine verdicts, newest first.

    "Own" is (squad, format): the pair is what identifies a worker in the
    ledger, since squad_merge_loop records the squad and the commit's
    Format trailer but not the worker id. A verdict that pins itself to no
    format (see _quarantine_entry_format: JSON null from overlord_sweep's
    squad bisection, or the "UNKNOWN" placeholder for a commit with no
    Format: trailer) matches every member of its squad -- it rolled back
    or rejected work on the branch they all share, so they all need it.

    Another squad's rejections are deliberately NOT shown, and neither
    are the same squad's rejections on a different format: they name
    parser files this worker never touches, and the budget they would
    spend is better spent on the module playbook.

    Ownership is decided by validate_fix_commit.squad_from_worker -- the
    SAME helper the merger uses to decide which squad a candidate commit
    belongs to -- so a label with no "-<n>" suffix is its own squad name
    and matches nothing in a ledger whose squads are real squad names.
    That is a deliberate fail-closed default, and it is the only safe one:
    the previous helper here returned None for such a label and the match
    degraded to FORMAT ALONE, which is not an edge case but the DEFAULT
    path -- parallel_model_fix_loop picks run_round unless --squad-mode,
    run_worker labels those workers with the bare format name ("JPEG",
    "CR2") and model_fix_loop's own CLI defaults the label to "1".
    Measured against the live ledger 2026-07-25, the degrade returned
    ps-docs/sony-minolta/thermal rejections for label "JPEG",
    exif-core/canon for "CR2", and ps-docs/sony-minolta/thermal for the
    bare "1" -- all rendered by format_own_quarantine under "YOUR OWN
    commits that were REJECTED ... Fix the defect named by the flags
    below". Showing nothing is strictly better than showing a lie.

    KNOWN GAP, stated plainly because the earlier version of this comment
    got it wrong. It is NOT true that a legacy worker owns no ledger
    entries: squad_merge_loop.candidate_worker_branches returns the legacy
    per-format branch model-fix-parallel-<fmt> for every format
    config.toml's [squads.*] tables list under a squad, and poll_once feeds those to
    process_commit(squad=<squad>, fmt=<fmt>), whose quarantine() records
    squad=<the CONSUMING squad>. So a legacy worker's commits do become
    entries -- filed under whichever squad consumed them, and since
    config.toml lists JPEG under 12 of 14 squads, potentially under many
    squad names at once. Nothing in the ledger ties them back to the
    legacy label the worker was given, so this function cannot claim them
    and returns [] on that path: the quarantine section is inert for
    legacy per-format workers, including the missing-trailer:Format class
    it would otherwise surface. The ledger does record `sha`, so ownership
    is recoverable in principle by walking the worker branch -- nothing
    does that today. Fail-closed is still the right default here; a wrong
    owner teaches a worker to fix someone else's defect.

    No worker_label at all returns [] before any of that: an absent label
    means there is no worker to be the owner of anything. That is what
    keeps build_prompt hermetic by default -- a caller that never opted
    in must not be shown some arbitrary worker's rejections.

    Missing/unreadable ledger, or no path: [] -- advisory context, never
    a hard dependency, same contract as every other build_prompt source.
    """
    if not quarantine_path or not worker_label:
        return []
    path = Path(quarantine_path)
    try:
        size = path.stat().st_size
    except OSError:
        return []
    offset = max(0, size - tail_kb * 1024)
    try:
        with path.open("rb") as f:
            f.seek(offset)
            data = f.read()
    except OSError:
        return []
    lines = data.split(b"\n")
    if offset > 0:
        lines = lines[1:]  # drop a possibly-partial leading fragment
    squad = squad_from_worker(str(worker_label).strip())
    entries = []
    for raw in lines:
        raw = raw.strip()
        if not raw:
            continue
        try:
            entry = json.loads(raw)
        except (UnicodeDecodeError, ValueError):
            continue
        if not isinstance(entry, dict):
            continue
        if entry.get("squad") != squad:
            continue
        entry_format = _quarantine_entry_format(entry)
        # A formatless verdict applies to every member of the squad.
        if entry_format and entry_format != format_name:
            continue
        entries.append(entry)
    entries.reverse()  # ledger is append-order (oldest first); newest first
    return entries[:max_entries]


def _clamp_quarantine_flags(raw_flags,
                            max_flags=QUARANTINE_MAX_FLAGS,
                            max_chars=QUARANTINE_FLAGS_DISPLAY_CHARS):
    """Render one entry's `flags` as a bounded, deterministic string.

    BOUNDED because the writer is unbounded: validate_fix_commit appends
    one `printconv-mismatch:<48-char excerpt>` flag per unverifiable map
    value with no cap of its own, so a commit adding a ~30-entry PrintConv
    lookup emits 30 flags in a single entry (the live worst case is
    already 11 flags / 512 chars, 2026-07-25). Four such entries rendered
    a 5858-char section against a 4800-char learning budget, and since
    LEARNING_SECTION_ORDER ranks this section SECOND, compose_learning_
    block then admitted the quarantine block ALONE and dropped the K4
    sweep reviews, the K3 module playbook and the lessons tail outright --
    the knowledge spine went dark for that worker for as long as the entry
    stayed in the 64KB window, which today is the whole ledger. The first
    few flags already name the defect class; flags 5..30 are the same
    class with different excerpts, so the tail costs budget and teaches
    nothing. The "(+N more)" suffix keeps the true count visible, and it
    counts flags dropped by EITHER cap -- the char budget decides how many
    flags fit, then the remainder is reported.

    DEFENSIVE because `flags` is written by another process. Every other
    field in this read path is coerced; this one was consumed as
    `", ".join(str(f) for f in entry["flags"])`, so a row whose flags is
    a bare int raised TypeError straight out of build_prompt, through
    fix_gap, and killed the worker -- from a section whose whole contract
    (read_own_quarantine's docstring) is "advisory context, never a hard
    dependency". A string is treated as ONE flag rather than as an
    iterable of characters, which is the only reading a writer could have
    meant; anything else non-list is reported as one opaque value.
    """
    if isinstance(raw_flags, str):
        flags = [raw_flags]
    elif isinstance(raw_flags, (list, tuple)):
        flags = [str(f) for f in raw_flags]
    elif not raw_flags:
        flags = []
    else:
        flags = [str(raw_flags)]
    flags = [f for f in flags if f]
    if not flags:
        return "(no flags)"
    # Both caps have to feed the SAME count, or "(+N more)" lies. Deriving
    # `hidden` from the flag-count cap alone meant
    # (['A'*100, 'B'*100, 'C'*100]) rendered 200 chars + "..." with
    # hidden == 0 -- the third flag vanished with nothing to say so. Decide
    # how many flags actually fit, then report the rest.
    shown = []
    used = 0
    for flag in flags[:max_flags]:
        cost = len(flag) + (2 if shown else 0)  # ", " between flags
        if shown and used + cost > max_chars:
            break
        shown.append(flag)
        used += cost
    if not shown:
        shown = [flags[0]]
    hidden = len(flags) - len(shown)
    text = ", ".join(shown)
    if len(text) > max_chars:
        # Only reachable when the FIRST flag alone exceeds the budget: it
        # is force-included above so the line is never empty, but the
        # bound this function exists to enforce still has to hold.
        text = text[:max_chars].rstrip() + "..."
    if hidden > 0:
        text += f" (+{hidden} more)"
    return text


def format_own_quarantine(entries):
    """Render read_own_quarantine's entries into a learning-block section.

    The flags lead each line because they ARE the verdict and they carry
    the offending value inline (validate_fix_commit emits
    "printconv-mismatch:<the wrong string>", "multi-sample-fail:<tag>",
    "missing-trailer:<key>"); the reason is the human-readable tail and
    is clamped hard. Deterministic, bounded output -- and now actually
    bounded, see _clamp_quarantine_flags: at most
    DEFAULT_QUARANTINE_MAX_ENTRIES lines, each at most
    QUARANTINE_FLAGS_DISPLAY_CHARS + QUARANTINE_REASON_DISPLAY_CHARS
    chars plus a fixed rendering overhead.
    """
    if not entries:
        return ""
    lines = []
    for entry in entries:
        flags = _clamp_quarantine_flags(entry.get("flags"))
        reason = _clamp_reason(entry.get("reason"), QUARANTINE_REASON_DISPLAY_CHARS)
        date = str(entry.get("ts") or "")[:10]
        stamp = f" [{date}]" if date else ""
        lines.append(f"  - {flags}{stamp}: {reason}")
    return (
        "\n\nYOUR OWN commits that were REJECTED at merge time (spec M2 "
        "quarantine ledger, newest first). These never reached the squad "
        "branch and were never retried -- anything you build on top of them "
        "will conflict or be rejected the same way. Fix the defect named by "
        "the flags below before repeating that approach:\n"
        + "\n".join(lines)
    )


# --- Section 6: adaptive response-format guidance, continued --------------
# (DIFF_FORMAT_FAILURE_RE and the escalation threshold are declared at the
# top of section 6 because select_module_lessons above depends on them.)

def count_diff_format_failures(events, worker_label):
    """How many of THIS worker's rows in the tail window failed with "no
    diff in model response". Scoped to the worker, not the module: the
    defect belongs to whichever model this worker is rotating through,
    and a sibling worker on the same module that formats its replies
    correctly must not be taxed for it. No worker_label (the hermetic
    default) means no escalation -- 0."""
    if not worker_label:
        return 0
    return sum(
        1 for ev in events
        if ev.get("worker") == worker_label
        and DIFF_FORMAT_FAILURE_RE.search(str(ev.get("reason") or ""))
    )


#: The corrective envelope statement. Deliberately concrete: the exact
#: fence, the exact minimum a unified diff needs to apply (git apply
#: rejects a bare @@ hunk with no ---/+++ headers), and one minimal
#: example that would actually apply -- a rule the model can pattern-match
#: against, not a rule it has to interpret.
#:
#: It must also be TRUE and must not contradict build_reply_shape_manifest,
#: which ships in the same prompt. The first version did both wrong: it
#: said the reply "is parsed by looking for a ```diff fenced block, and
#: nothing else counts" and that prose is "discarded unread", while the
#: manifest defines four legal shapes of which shape 1 (REQUEST) is a bare
#: prose line with no diff and shape 4 REQUIRES 2-3 sentences of prose
#: before the fence -- and the STRATEGY paragraph tells the worker to probe
#: with VERIFY. A worker that believed the alert would stop issuing
#: REQUEST/VERIFY, i.e. the alert would cost rounds instead of saving them.
#: Two of its claims were also simply false, both in the strict direction:
#: extract_diff ALSO accepts an unfenced reply starting with "diff --git"/
#: "--- ", and DIFF_BLOCK_RE is a non-greedy search, so text after the
#: closing fence is ignored rather than fatal. Every claim below is pinned
#: to extract_diff's real behaviour by
#: AdaptiveDiffFormatGuidanceTests.test_the_alert_makes_no_claim_
#: extract_diff_contradicts.
DIFF_FORMAT_REMEDIATION = """\
FORMAT ALERT -- one or more of your recent rounds ended with no diff \
extracted from your reply, so the round was wasted. This was NOT a code \
problem, it is how the reply was packaged.

The four reply shapes at the top of this prompt all still apply, and this \
alert does not narrow them: REQUEST and VERIFY remain the right moves when \
you are still investigating, and a Plan + diff reply is SUPPOSED to open \
with 2-3 sentences of prose before the fence. What does not work is ending \
a round with prose ALONE when you meant to deliver a change.

Whenever your reply is meant to carry a change, it must contain the change \
as a unified diff in EXACTLY ONE ```diff fenced block, complete, with \
per-file ---/+++ headers -- a bare @@ hunk has nothing to apply against and \
is rejected. A ```rust or ```patch fence is not read as a diff, and a diff \
split across two fences loses everything after the first fence closes. \
Minimal correct example:

```diff
--- a/src/parsers/jpeg/foo.rs
+++ b/src/parsers/jpeg/foo.rs
@@ -10,6 +10,7 @@ fn parse(&self) {
     let a = 1;
+    let b = 2;
     let c = 3;
```

Text after the closing fence is ignored, so a closing sentence is harmless; \
a SECOND ```diff block is not, because only the first is read. If the diff \
is too large for one reply, use the "PATCH i/N" shape from the manifest \
above -- each chunk is still ONE ```diff fenced block."""


def build_diff_format_remediation(failure_count,
                                  threshold=DEFAULT_DIFF_FORMAT_ESCALATION_THRESHOLD):
    """The corrective envelope block for a worker that has recently hit
    "no diff in model response", "" for one that hasn't (see
    count_diff_format_failures for why this is per-worker)."""
    if failure_count < threshold:
        return ""
    return (
        f"\n\n{DIFF_FORMAT_REMEDIATION}\n(Triggered by {failure_count} recent "
        "reply/replies of yours from which no diff could be extracted.)"
    )


# --- Section 6: learning-block composition ---------------------------------

#: The learning block's fixed priority order, highest first. Whatever
#: overflows learning_budget_tokens is shed from the TAIL of this list:
#:
#:   1. diff-format remediation -- tiny, adaptive (only present for a
#:      worker that needs it), and targets ~40% of all recorded failures.
#:      Useless if truncated, so it goes first.
#:   2. the worker's own quarantine verdicts -- the only section that is
#:      about THIS worker's own rejected code; without it the worker
#:      rebuilds on top of it.
#:   3. sweep reviews (K4) -- human verdicts, the scarcest signal there is.
#:   4. module playbook (K3) -- the distiller's recurrence-ranked digest.
#:   5. lessons tail (K1) -- raw rows, the most redundant with 4.
#:
#: 3/4/5 keep the order they had before 1/2 existed, so a worker with
#: neither a quarantine record nor a format problem gets a byte-identical
#: learning block to the one it got before (prompt-cache friendly, and
#: the reason every pre-existing build_prompt test still passes unchanged).
LEARNING_SECTION_ORDER = (
    "diff_format", "quarantine", "sweep_reviews", "module_playbook", "lessons_tail",
)


def compose_learning_block(parts, budget_tokens):
    """Assemble the learning block from `parts` ({name: text}) in
    LEARNING_SECTION_ORDER and clamp the result to budget_tokens.

    Each section is admitted whole while the budget allows; the first one
    that doesn't fit is clamped to exactly what remains and every later
    section is dropped. That makes the budget a hard bound no matter how
    large the ledgers grow -- a 100k-row lessons.jsonl produces the same
    number of prompt bytes as a 100-row one -- while guaranteeing that
    the sections most likely to change a worker's behaviour are the ones
    that survive the squeeze.

    Unknown keys in `parts` are ignored rather than appended in dict
    order: a section that isn't ranked has no defined position under
    pressure, and silently giving it one is how an unranked section ends
    up starving a ranked one.
    """
    budget_chars = max(0, budget_tokens) * 4
    out = []
    used = 0
    for name in LEARNING_SECTION_ORDER:
        text = parts.get(name) or ""
        if not text:
            continue
        remaining = budget_chars - used
        if remaining <= 0:
            break
        out.append(text[:remaining])
        used += min(len(text), remaining)
    return "".join(out)


# --- prompt-cache section ordering (2026-07-26) -----------------------------
#
# The fleet's worker pool now leads with deepseek/deepseek-v4-pro on
# OpenRouter: input $0.435/M, output $0.87/M, CACHE READ $0.0036/M -- a
# 120x discount on any leading run of tokens that is byte-identical to a
# recent request. DeepSeek's cache is AUTOMATIC PREFIX caching: there is
# no cache_control to place (apply_prompt_cache_markers' Anthropic-style
# breakpoints are a no-op here, which is why config ships
# prompt_cache = "auto"), so the ONLY lever is the order the sections are
# rendered in. One varying byte ends the prefix and everything after it is
# re-billed at 120x.
#
# The order below is derived from measurement, not intuition: 216 renders
# of real gap dicts reconstructed from the fleet's own saved fixer
# requests (~/.oxidex/logs/model-fix-requests), covering 22 formats x 6
# gaps x 2 worker ids, with the live samples dir / ExifTool Perl lib /
# knowledge home wired in. Per section: distinct rendered values across
# ALL renders, mean distinct values WITHIN one format, and whether it
# changes when only the worker id changes:
#
#   section        mean chars   distinct/all   distinct/format   per-worker
#   constraints          1785              1              1.00          no
#   pitfalls             5103              1              1.00          no
#   manifest             1917              1              1.00          no
#   primer               1363              1              1.00          no
#   format line            86             22              1.00          no
#   samples               174             20              1.00          no
#   parser_files        28707             24              1.64          no
#   perl NOTES           1363             19              1.64          no
#   learning             3223             40              2.00         YES
#   missing+diffs         ~90            104              4.73          no
#   exact_sample         2337             25              1.86          no
#   perl_block            791             83              4.27          no
#   tail                  331              1              1.00          no
#
# The first four rows were one section ("intro") together with the format
# line and the missing-tag list, which is why the old order lost the
# prefix at ~1.8 KB (cross-format) / ~7 KB (same format) out of a ~17.5 KB
# prompt: the single most volatile string in the whole prompt sat at byte
# 1871, in front of the 28 KB of parser source. Splitting it is the
# single highest-value change here.
#
# Two placements are deliberate and worth not "fixing" later:
#   * learning ABOVE the gap list. It reads as per-worker orientation, and
#     the measurement says it is genuinely more stable than the gap list:
#     for a fixed worker it was identical across all 6 tags of a format
#     (2.00 distinct/format == exactly the 2 worker ids probed), while the
#     gap list changed on every tag (4.73 distinct/format). Workers
#     process tags back to back, so this is the common case.
#   * exact_sample ABOVE the gap list, which is where it did NOT used to
#     be. Different tags of one format usually come from the same sample
#     file (1.86 distinct values per format, against the gap list's 4.73),
#     so it belongs in the more stable tier -- worth 81.6% -> 87.6% in the
#     offline harness. Its lead-in was reworded from "this exact tag" to
#     "the tag targeted below" so the reference still resolves.
#   * perl_block / neighbor BELOW the gap list, where they already were.
#     Same argument would move perl_block up, but it is worth only ~0.2
#     points (791 mean chars, 4.27 distinct/format -- it tracks the tag
#     almost exactly) and its lead-in text is shared with the critique and
#     foundation-job prompts, where the tags ARE listed above it. Not
#     worth rewording a string three prompts depend on.
#
# Measured end to end over the same 22-format corpus with max_prompt_
# tokens=4096: 40.1% -> 87.6% of a prompt cacheable against another prompt
# for the same format, and 1864 -> 8133 bytes cacheable against a prompt
# for a DIFFERENT format (i.e. across the whole fleet, not just one
# worker) -- see the ordering-variant sweep recorded in the commit body.
PROMPT_SECTION_ORDER = (
    "invariants",     # tier 0: identical for every worker/format/tag
    "format_intro",   # tier 1: per format
    "samples",        # tier 1: per format
    "parser_files",   # tier 2: per format, occasionally per tag
    "overview",       # tier 2: per format, occasionally per tag
    "learning",       # tier 3: per worker
    "exact_sample",   # tier 3: per sample file, shared across sibling tags
    "gaps",           # tier 4: per tag -- the volatile payload
    "perl_block",     # tier 5: per tag, must follow the gap list
    "neighbor",       # tier 5: per tag, must follow the gap list
    "attempts",       # tier 6: append-only, grows every failed round
    "tail",           # invariant, but its whole job is to be last
)

#: Which sections assemble_prompt_sections may shrink when a prompt
#: overflows max_prompt_tokens, in the order it sheds them: least
#: essential first. This is a SEPARATE axis from PROMPT_SECTION_ORDER
#: above and must stay that way -- render order is chosen for cache
#: prefix length, shrink order for what the fixer can most afford to
#: lose. They used to be the same tuple by accident (one `sections` list
#: whose order fed both), so reordering for cache would silently have
#: started shedding parser source before attempt history.
#: assemble_prompt_sections reads render order from its `sections`
#: argument and shrink order from its `budgets` argument, and never
#: conflates them; test_shrink_priority_is_independent_of_render_order
#: pins that.
PROMPT_SHRINK_PRIORITY = (
    "attempts", "samples", "neighbor", "perl_block", "parser_files",
)


def build_prompt(gap, repo_root=REPO_ROOT, max_tags=DEFAULT_MAX_PROMPT_TAGS,
                  max_file_bytes=DEFAULT_MAX_PROMPT_FILE_BYTES, samples_dir=None,
                  max_samples_listed=DEFAULT_MAX_SAMPLE_FILES_LISTED, previous_attempts=None,
                  perl_lib_dir=None, sweep_review_log_path=None,
                  max_prompt_tokens=DEFAULT_MAX_PROMPT_TOKENS,
                  max_reply_tokens=DEFAULT_MAX_TOKENS,
                  neighbor_precedent_block="",
                  knowledge_home=None, module_name=None,
                  learning_budget_tokens=DEFAULT_LEARNING_BUDGET_TOKENS,
                  parser_floor_tokens=DEFAULT_PARSER_FLOOR_TOKENS,
                  lessons_tail_kb=DEFAULT_LESSONS_TAIL_KB,
                  worker_label=None):
    """Format one gap into a model prompt, capped so a huge format (e.g.
    JPEG with thousands of gaps and dozens of parser files) becomes an
    iterative, tractable request instead of one impossibly large prompt.
    Whatever's omitted here resurfaces in a later round automatically,
    since gap["gap_count"] (used by fix_gap's verification) always
    reflects the format's real total, not just what's shown below.

    samples_dir, if given, is scanned for real sample files matching this
    format (case-insensitive filename suffix) and a handful are listed so
    the model can ask to see one's actual raw bytes via the REQUEST:
    protocol (see attempt_build) instead of guessing at binary layout from
    tag names/values alone.

    max_prompt_tokens bounds THIS prompt (the input); max_reply_tokens is
    the separate config["max_tokens"] cap on the model's own REPLY, passed
    through to build_reply_shape_manifest so the PATCH-i/N-chunking
    threshold it states matches the cap that will actually truncate a
    reply, rather than the input budget above.

    previous_attempts, if given, is this tag's persisted attempt history
    (run_tag_loop's per-tag "attempts" list) -- see format_previous_attempts.

    perl_lib_dir, if given (see resolve_exiftool_perl_lib_dir), is used to
    include ExifTool's own Perl source for this gap's tags -- see
    build_perl_reference_block. None (the default) omits this section
    entirely rather than resolving it implicitly, so callers that don't
    pass it (including every existing test) keep their original,
    environment-independent output.

    sweep_review_log_path, if given, is used to include recent human
    sweep-review verdicts (spec K4: same-format entries plus cross-format
    REJECTIONS, see load_recent_sweep_reviews/format_sweep_review_history)
    for this gap's format. None (the default) omits this section, same
    reasoning as perl_lib_dir.

    knowledge_home, if given, is the OXIDEX_HOME-style directory the
    shared knowledge layer lives under (spec K2/K3): it gates BOTH the
    curated <knowledge_home>/logs/knowledge/GLOBAL-PITFALLS.md (replacing
    the bare KNOWN_PITFALLS constant -- see load_global_pitfalls) and the
    module playbook + lessons-tail sections below. None (the default)
    keeps every existing caller's output byte-identical AND keeps this
    function hermetic (it never touches the real OXIDEX_HOME unless a
    caller opts in) -- exactly like perl_lib_dir/sweep_review_log_path.

    module_name, if given, selects which
    <knowledge_home>/logs/knowledge/modules/<module_name>.md playbook to
    show (spec K3); None falls back to gap["format"] as the key. Also
    scopes the lessons-tail selection (module match takes priority over
    format match -- see select_module_lessons). Has no effect when
    knowledge_home is None.

    learning_budget_tokens/parser_floor_tokens/lessons_tail_kb are the
    section-6 knobs: the learning block (diff-format remediation + this
    worker's own quarantine verdicts + sweep reviews + module playbook +
    lessons tail -- everything knowledge_home gates, plus
    sweep_review_log_path's section) is capped at learning_budget_tokens
    and never dropped entirely; the parser-files section never shrinks
    below parser_floor_tokens even under the worst overflow;
    lessons_tail_kb bounds how much of the tail of logs/lessons.jsonl
    read_lessons_tail_events seeks into. Within the learning block,
    overflow is shed from the tail of LEARNING_SECTION_ORDER (see
    compose_learning_block), so the budget bounds a growing ledger
    without letting the low-value sections squeeze out the high-value
    ones.

    worker_label, if given, is this worker's fleet id (e.g. "canon-3" --
    parallel_model_fix_loop's f"{squad}-{n}"). It gates the two
    per-worker learning sections, both of which need to know WHOSE
    history is being read: the quarantine verdicts for this worker's own
    (squad, format) -- see read_own_quarantine -- and the adaptive
    response-format remediation for a worker that has recently been
    losing rounds to "no diff in model response" (see
    count_diff_format_failures). None (the default) omits both, so every
    existing caller's prompt is byte-identical. Like the rest of the
    learning block it does nothing unless knowledge_home is also given
    -- there is no ledger to read otherwise.

    neighbor_precedent_block is a pre-rendered string (see
    build_neighbor_precedent_block, built by the caller so build_prompt
    itself stays free of subprocess calls) inserted after the Perl
    reference in the stable per-tag section; "" (the default) omits it.

    Sections render in PROMPT_SECTION_ORDER: most-stable-first, so the
    byte-identical leading run a provider's automatic prefix cache can
    reuse is as long as possible (see that constant for the measured
    per-section stability the order is derived from, and for why
    `learning` sits above the gap list while the per-tag reference
    blocks sit below it).

    Section 6: overflow beyond max_prompt_tokens is shed via graduated
    per-section truncation (see assemble_prompt_sections) rather than
    plain head-keeping -- attempts, then samples, then neighbor
    precedent, then perl_block, then the parser-files section down to
    (never below) parser_floor_tokens, in that priority order
    (PROMPT_SHRINK_PRIORITY, which is deliberately independent of the
    render order); the learning block is never part of that squeeze (it
    gets its own flat, never-emptied budget instead).
    """
    missing_shown = gap["missing_tags"][:max_tags]
    missing_omitted = len(gap["missing_tags"]) - len(missing_shown)
    missing = "\n".join(
        f"  - {t['family']}:{t['name']} = {t['value']} (sample: {t.get('source_file') or 'n/a'})"
        for t in missing_shown
    ) or "  (none)"
    if missing_omitted > 0:
        missing += f"\n  ... and {missing_omitted} more, not shown (will resurface in a later round)"

    diffs_shown = gap["value_differences"][:max_tags]
    diffs_omitted = len(gap["value_differences"]) - len(diffs_shown)
    diffs = "\n".join(
        f"  - {d['tag_key']}: exiftool=\"{d['exiftool_value']}\" oxidex=\"{d['oxidex_value']}\" (sample: {d['source_file']})"
        for d in diffs_shown
    ) or "  (none)"
    if diffs_omitted > 0:
        diffs += f"\n  ... and {diffs_omitted} more, not shown (will resurface in a later round)"

    file_blocks = []
    bytes_used = 0
    files_omitted = 0
    for f in gap["parser_files"]:
        try:
            content = (repo_root / f).read_text()
        except OSError:
            continue
        if bytes_used + len(content) > max_file_bytes and file_blocks:
            files_omitted += 1
            continue
        file_blocks.append(f"--- {f} ---\n{content}")
        bytes_used += len(content)
    files = "\n\n".join(file_blocks) or "(no parser files located -- search src/ yourself)"
    if files_omitted > 0:
        files += f"\n\n({files_omitted} additional file(s) omitted to keep this prompt a reasonable size)"

    samples_block = ""
    if samples_dir is not None:
        exts = FORMAT_SAMPLE_EXTENSIONS.get(gap["format"], [gap["format"].lower()])
        sample_paths = sorted(
            p for ext in exts for p in Path(samples_dir).rglob(f"*.{ext}")
        )[:max_samples_listed]
        if sample_paths:
            listed = "\n".join(f"  - {p.relative_to(samples_dir)}" for p in sample_paths)
            samples_block = (
                f"\n\nReal sample files available for this format (relative to the samples dir):\n{listed}\n"
                "(REQUEST one -- shape 1 above -- to get a hex dump of its raw bytes.)"
            )

    exact_sample_block = build_exact_sample_block(gap, samples_dir)

    perl_block = build_perl_reference_block(gap, perl_lib_dir)

    # Split, not build_format_overview_block: the invariant primer half
    # goes in the tier-0 section and only these per-tag NOTES stay down
    # here. See build_perl_notes_block and PROMPT_SECTION_ORDER.
    perl_notes_block = build_perl_notes_block(perl_lib_dir, perl_block)

    # Spec K2: fresh read every call, falls back to the KNOWN_PITFALLS
    # constant when knowledge_home is omitted (hermetic by default -- see
    # the docstring above) or the file is missing/blank.
    pitfalls_text = load_global_pitfalls(knowledge_home) if knowledge_home is not None else KNOWN_PITFALLS

    sweep_review_block = ""
    if sweep_review_log_path is not None:
        sweep_review_block = format_sweep_review_history(
            load_recent_sweep_reviews(sweep_review_log_path, gap["format"])
        )

    # Spec K3: module playbook, replacing load_format_memory. Format-name
    # fallback key when module attribution is ambiguous/unavailable.
    module_block = ""
    if knowledge_home is not None:
        module_key = module_name or gap["format"]
        playbook_text = load_module_playbook(knowledge_home, module_key)
        if playbook_text:
            module_block = (
                "\n\nModule playbook (distilled cross-worker lessons for this gap's "
                f"module -- {module_key} -- see scripts/distill_lessons.py):\n\n"
                + playbook_text
            )

    # Section 6: the three ledger-fed sections. All read the SAME bounded
    # tail bytes once (read_lessons_tail_events), then scope them
    # differently -- by module/format for the ranked lessons, by worker
    # for the diff-format detector -- rather than seeking into a
    # multi-megabyte ledger twice per prompt.
    lessons_tail_block = ""
    diff_format_block = ""
    quarantine_block = ""
    if knowledge_home is not None:
        tail_events = read_lessons_tail_events(
            Path(knowledge_home) / "logs" / "lessons.jsonl", tail_kb=lessons_tail_kb,
        )
        lessons_tail_block = format_lessons_tail(select_module_lessons(
            tail_events, module_name, gap["format"],
        ))
        diff_format_block = build_diff_format_remediation(
            count_diff_format_failures(tail_events, worker_label),
        )
        # squad_merge_loop.quarantine_ledger_path's layout, resolved
        # inline: this module is a reader of that ledger, not a
        # participant in the merge protocol, and importing the whole
        # merger to spell one path would drag its git/subprocess surface
        # into every worker prompt build.
        quarantine_block = format_own_quarantine(read_own_quarantine(
            Path(knowledge_home) / "logs" / "quarantine.jsonl",
            worker_label, gap["format"],
        ))

    # Spec section 6: the learning block (the pitfalls excerpt is NOT part
    # of it -- that sits in the tier-0 "invariants" section for
    # cache-prefix reasons, see PROMPT_SECTION_ORDER; only the remaining
    # pieces share this reserved, never-emptied budget) is capped flat,
    # independent of whatever else is squeezing the rest of the prompt,
    # and shed from the tail of LEARNING_SECTION_ORDER when it overflows.
    learning_text = compose_learning_block({
        "diff_format": diff_format_block,
        "quarantine": quarantine_block,
        "sweep_reviews": sweep_review_block,
        "module_playbook": module_block,
        "lessons_tail": lessons_tail_block,
    }, learning_budget_tokens)

    attempts_block = format_previous_attempts(previous_attempts)

    manifest = build_reply_shape_manifest(max_prompt_tokens, max_reply_tokens)
    texts = {
        # Tier 0 -- byte-identical for every worker, every format, every
        # tag, forever. Everything downstream of a single varying byte is
        # re-billed at full input price, so this is the whole prefix's
        # foundation and nothing interpolated may leak into it.
        "invariants": (
            f"{RUST_ARCHITECTURE_CONSTRAINTS}\n\n"
            f"{pitfalls_text}\n\n"
            f"{manifest}\n\n"
            f"{ARCHITECTURE_PRIMER}"
        ),
        # Tier 1 -- per FORMAT, identical for every tag in it. No trailing
        # newline: every section below opens with its own "\n\n", and a
        # section that renders empty must not leave a ragged blank run.
        "format_intro": (
            "\n\nYou are fixing ExifTool tag-coverage gaps in the oxidex Rust "
            f"codebase, format \"{gap['format']}\"."
        ),
        "samples": samples_block,
        # Tier 2 -- per format, but a tag in an unusual module/subtree can
        # pull in a different file set or a different Perl table's NOTES.
        # The "Likely relevant source files:" label rides on parser_files
        # (not on the NOTES above it) so the two stay adjacent no matter
        # which is empty, and so a floor-clamped parser_files still keeps
        # its introduction -- see PROMPT_SECTION_ORDER.
        "parser_files": f"\n\nLikely relevant source files:\n{files}",
        "overview": perl_notes_block,
        # Tier 3 -- per worker: same ledger for every tag this worker
        # takes, so it is MORE stable than the gap list below it.
        "learning": learning_text,
        # Tier 3 -- per sample FILE, which sibling tags share; its lead-in
        # points forwards at the gap list rather than back at it.
        "exact_sample": exact_sample_block,
        # Tier 4 -- the volatile payload: what actually changes every call.
        "gaps": (
            f"\n\nMissing entirely (ExifTool extracts it, oxidex doesn't):\n{missing}\n\n"
            f"Value differences (both extract it, values disagree):\n{diffs}"
        ),
        # Tier 5 -- per tag, and referring back to the gap list above
        # ("these tags", the neighbouring-tag precedent).
        "perl_block": perl_block,
        "neighbor": neighbor_precedent_block,
        # Tier 6 -- append-only, grows with every failed round.
        "attempts": attempts_block,
        "tail": (
            "\n\nFor value differences, only fix genuine bugs, not benign formatting differences. "
            "If more gaps exist than are shown above, that's expected -- fix what's shown here; "
            "future rounds will address the rest.\n\n"
            f"{TERMINAL_REMINDER}"
        ),
    }
    sections = [(name, texts[name]) for name in PROMPT_SECTION_ORDER]
    # Section 6 shrink-priority order, deliberately NOT the render order
    # above: attempts, then samples, then neighbor precedent, then
    # perl_block, then parser files down to (never below)
    # parser_floor_tokens. "learning" is deliberately absent -- its own
    # flat cap above already gives it the "reserved, never dropped
    # entirely" guarantee independent of this squeeze.
    floors = {
        "attempts": 0,
        "samples": 0,
        "neighbor": 0,
        "perl_block": 0,
        "parser_files": parser_floor_tokens,
    }
    budgets = {name: floors[name] for name in PROMPT_SHRINK_PRIORITY}
    return assemble_prompt_sections(sections, budgets, max_prompt_tokens)


FORMAT_SAMPLE_EXTENSIONS = {
    "JPEG": ["jpg", "jpeg"],
    "TIFF": ["tif", "tiff"],
    "HEIC": ["heic", "heif"],
    "PNG": ["png"],
    "GIF": ["gif"],
    "PDF": ["pdf"],
    "MP4": ["mp4", "mov", "m4v"],
    "WEBP": ["webp"],
    "BMP": ["bmp"],
    "PSD": ["psd"],
    "AVIF": ["avif"],
}


#: K5/section 6: independent of the fixer's max_prompt_tokens.
DEFAULT_REVIEWER_MAX_PROMPT_TOKENS = 8192

#: Section 6 checklist, verbatim (maps 1:1 to the human rejection
#: taxonomy): C1/C2 are class (a) wrong-value mistakes, C3 is class (b)
#: double emission, C4 is class (c) fixture invention, C5 is the general
#: hardcoded-sample-value smell. Checklist ids flow into lessons.jsonl as
#: the clusterable fingerprint key (see parse_checklist_id).
REVIEW_CHECKLIST = """
C1: exact tag ID / table index matches the Perl shown (class a).
C2: PrintConv strings byte-identical, not paraphrased (class a).
C3: the diff edits an emitter found in the emission scan rather than adding a second path (class b).
C4: any new/changed test asserts values from a real corpus sample, not a fixture invented in this diff (class c).
C5: no hardcoded sample-specific values.
""".strip()

_CHECKLIST_ID_RE = re.compile(r"\b(C[1-5])\b", re.IGNORECASE)


def parse_checklist_id(reason):
    """The first "C1".."C5" checklist token mentioned in a REJECT/
    UNVERIFIABLE reason string (spec section 6), or None. Shared by K1's
    review_rejected lesson event and K5's UNVERIFIABLE routing."""
    m = _CHECKLIST_ID_RE.search(str(reason or ""))
    return m.group(1).upper() if m else None


def build_review_prompt(gap, diff, perl_block="", live_evidence="", emission_scan="",
                        max_tokens=DEFAULT_REVIEWER_MAX_PROMPT_TOKENS):
    """K5: the reviewer prompt, now carrying the same Perl reference the
    fixer saw (perl_block), a live post-fix re-extraction (live_evidence
    -- NOT the comparison JSON, whose matched_tags carries no values),
    and a scoped emission scan (emission_scan -- rg over this format's
    parser subtree, not a repo-wide grep), plus the C1-C5 checklist with
    its mandatory APPROVE/REJECT/UNVERIFIABLE reply shape. All three
    evidence params default to "" (omitted) so callers that don't have
    them (including every existing test) keep prior, environment-
    independent output.
    """
    missing_names = ", ".join(
        f"{t['family']}:{t['name']}" for t in gap["missing_tags"][:10]
    ) or "(none)"
    diff_names = ", ".join(
        d["tag_key"] for d in gap["value_differences"][:10]
    ) or "(none)"
    perl_section = f"\n\nExifTool Perl reference (the same snippets the fixer saw):\n{perl_block}" if perl_block else ""
    evidence_section = f"\n\nLive re-extraction, post-fix (exiftool vs oxidex on the real sample):\n{live_evidence}" if live_evidence else ""
    emission_section = f"\n\nEmission scan (every place this tag is emitted in the format's parser subtree):\n{emission_scan}" if emission_scan else ""
    prompt = (
        f"You are reviewing a proposed fix for ExifTool tag-coverage gaps in the oxidex Rust codebase, "
        f"format \"{gap['format']}\". The fix was supposed to address (among possibly more): "
        f"missing tags [{missing_names}], value differences [{diff_names}].\n\n"
        f"Here is the diff that was applied and successfully built:\n\n{diff}\n\n"
        "Judge whether this is a genuine, general implementation of the missing tag parsing/serialization "
        "logic, or whether it games the specific sample file it was tested against -- for example, "
        "hardcoding a literal expected value instead of actually decoding it, special-casing a filename, "
        "or any other shortcut that would only work for the one file used to verify this fix."
        f"{perl_section}{evidence_section}{emission_section}\n\n"
        "Checklist -- answer each item briefly, THEN give your verdict:\n\n"
        f"{REVIEW_CHECKLIST}\n\n"
        "Respond with exactly one of:\n"
        "APPROVE\n"
        "or\n"
        "REJECT: <Cn> <reason>\n"
        "or\n"
        "UNVERIFIABLE: <Cn> <reason -- what evidence you'd need but don't have, e.g. the relevant "
        "Perl table didn't fit the prompt budget>"
    )
    return truncate_to_token_budget(prompt, max_tokens)


def extract_review_verdict_full(response_text):
    """K5: parse a reviewer response into the full three-way verdict --
    ("approve" | "reject" | "unverifiable", reason). Unparseable
    responses are "reject" -- fail-safe, never silently approve something
    we couldn't understand. See extract_review_verdict for the
    preserved, backward-compatible two-tuple shape existing callers use."""
    stripped = response_text.strip()
    verdict = _verdict_from_line(stripped)
    if verdict is not None:
        return verdict

    # The review prompt itself instructs "answer each checklist item
    # briefly, THEN give your verdict", so a model that FOLLOWS the
    # instruction puts its verdict on the LAST line, not the first --
    # and the first-line-only match above then scored it unparseable,
    # which fails safe to REJECT. Measured live: 7 of 209 reviewer
    # replies (3.3%) were APPROVE verdicts inverted to REJECT this way,
    # destroying ~4 already-built, already-gap-verified fixes against 10
    # delivered in the same window.
    #
    # Scanning bottom-up (not top-down) is deliberate: a checklist body
    # routinely mentions the words approve/reject while discussing the
    # criteria, so the LAST such line is the model's actual conclusion.
    # A response with no verdict line anywhere still falls through to
    # reject -- the fail-safe posture is preserved, just no longer
    # triggered by correct answers.
    for line in reversed(stripped.splitlines()):
        verdict = _verdict_from_line(line.strip())
        if verdict is not None:
            return verdict
    return "reject", f"unparseable review verdict: {stripped[:200]!r}"


# Tolerated decoration around a verdict line, e.g. "**Final Verdict:** APPROVE"
# or "Verdict: REJECT: C3 ..." -- stripped before the keyword match below.
_VERDICT_PREFIX_RE = re.compile(r"^[*_`\s>#-]*(?:final\s+)?verdict\s*:?\s*", re.IGNORECASE)


def _verdict_from_line(line):
    """(verdict, reason) for one line that states a verdict, else None.

    Shared by the first-line fast path and the bottom-up rescan so both
    accept exactly the same shapes -- including a "Verdict:" label and
    light markdown emphasis, which reviewers emit routinely.
    """
    if not line:
        return None
    candidate = _VERDICT_PREFIX_RE.sub("", line).lstrip("*_`# ").strip()
    upper = candidate.upper()
    if upper.startswith("APPROVE"):
        return "approve", ""
    if upper.startswith("UNVERIFIABLE"):
        _, _, reason = candidate.partition(":")
        return "unverifiable", reason.strip() or "unverifiable, no checklist id given"
    if upper.startswith("REJECT"):
        _, _, reason = candidate.partition(":")
        return "reject", reason.strip() or "rejected, no reason given"
    return None


def extract_review_verdict(response_text):
    """Parse a review response into (approved, reason) -- the ORIGINAL
    two-tuple contract, preserved byte-for-byte for existing callers.
    Delegates to extract_review_verdict_full (K5's three-way vocabulary):
    "unverifiable" degrades to "not approved" here, same as "reject",
    since a plain boolean has no way to represent the third state.
    Callers that need to route UNVERIFIABLE to the human queue instead of
    a hard rejection use review_verdict (or extract_review_verdict_full
    directly), not this function."""
    verdict, reason = extract_review_verdict_full(response_text)
    return verdict == "approve", reason


def review_verdict(gap, diff, config, call_model_fn=call_model, pick_model_fn=random.choice,
                   perl_block="", live_evidence="", emission_scan=""):
    """Ask the model to review a diff for genuineness (not gaming the
    sample file).

    pick_model_fn(models) -> model_spec selects which of config["models"]
    to use for this call; defaults to a random pick, so a run with multiple
    reviewer models rotates across the pool one call at a time. Each spec is
    a {"name", "base_url", "api_key"} dict -- pool entries may span
    different providers, not just different model names on the same one.
    Injectable for deterministic tests.

    perl_block/live_evidence/emission_scan (spec K5), each "" by default,
    are passed straight through to build_review_prompt.

    Returns (approved, reason) -- the same two-tuple every caller (fix_gap
    included) already expects. UNVERIFIABLE replies are folded into
    approved=True (spec: "the fix STILL lands") with reason prefixed
    "UNVERIFIABLE: " so fix_gap can detect it and populate review_flags/
    the Review-Unverifiable trailer for C1/C2 -- or, when the reply
    omits a parseable Cn token entirely, UNKNOWN (fail-safe: unknown
    severity still routes to the human queue rather than silently
    passing) -- see extract_review_verdict_full/parse_checklist_id,
    without fix_gap needing its own reviewer-calling contract change.
    """
    prompt = build_review_prompt(
        gap, diff, perl_block=perl_block, live_evidence=live_evidence, emission_scan=emission_scan,
        max_tokens=config.get("reviewer_max_prompt_tokens", DEFAULT_REVIEWER_MAX_PROMPT_TOKENS),
    )
    model_spec = pick_model_fn(config["models"])
    try:
        reply = call_model_fn(
            [{"role": "user", "content": prompt}],
            model_spec["base_url"], model_spec["api_key"], model_spec["name"],
            config["max_tokens"], model_spec.get("reasoning_effort") or config["reasoning_effort"],
            config.get("stream", False), config.get("thinking", True),
            config.get("temperature", 0), config.get("timeout", 120),
            config.get("max_retries", DEFAULT_MAX_RETRIES),
            config.get("retry_backoff_seconds", DEFAULT_RETRY_BACKOFF_SECONDS),
            config.get("max_retry_backoff_seconds", DEFAULT_MAX_RETRY_BACKOFF_SECONDS),
        )
    except Exception as e:
        # Includes ModelQuotaExhausted: a reviewer that cannot be reached
        # must not kill the worker. This is "not approved this round", not
        # a judgement on the diff -- the tag comes back around and gets
        # reviewed again once the provider is answering.
        return False, f"review call failed: {e}"
    verdict, reason = extract_review_verdict_full(reply)
    if verdict == "approve":
        return True, ""
    if verdict == "unverifiable":
        return True, f"UNVERIFIABLE: {reason}"
    return False, reason


def build_failure_critique_prompt(gap, diff, failure_kind, failure_detail):
    """failure_kind is one of "build_failed", "gap_not_closed",
    "test_regressed", "review_rejected" -- diff may be None (e.g. the
    model never produced one)."""
    diff_block = f"\n\nThe diff that was attempted:\n{diff}" if diff else "\n\n(No diff was produced this attempt.)"
    return (
        f"A fixer's attempt to close ExifTool tag-coverage gaps for format \"{gap['format']}\" "
        f"failed at the \"{failure_kind}\" stage.\n\nWhat happened: {failure_detail}"
        f"{diff_block}\n\n"
        "In 2-3 sentences, explain the most likely root cause and what the fixer should try "
        "differently next attempt. Be specific and actionable (name the exact function/tag/"
        "assumption to reconsider if you can) -- this critique is shown directly to the fixer "
        "before its next try, and persists into future rounds' context even if this exact tag "
        "isn't retried again this session."
    )


def critique_failed_attempt(gap, diff, failure_kind, failure_detail, config,
                             call_model_fn=call_model, pick_model_fn=random.choice):
    """Get a reviewer-style critique of a failed (not just review-rejected)
    attempt, for course-correction context on the next round. Always
    returns a critique string -- falls back to failure_detail itself if
    the critique call fails, since a raw compiler/test error is still
    more useful feedback than nothing, and a critique-generation failure
    must never be allowed to abort the fixer's own retry loop.
    """
    try:
        model_spec = pick_model_fn(models_for_phase(config["models"], "explore"))
        prompt = build_failure_critique_prompt(gap, diff, failure_kind, failure_detail)
        reply = call_model_fn(
            [{"role": "user", "content": prompt}],
            model_spec["base_url"], model_spec["api_key"], model_spec["name"],
            config["max_tokens"], model_spec.get("reasoning_effort") or config["reasoning_effort"],
            config.get("stream", False), config.get("thinking", True),
            config.get("temperature", 0), config.get("timeout", 120),
            config.get("max_retries", DEFAULT_MAX_RETRIES),
            config.get("retry_backoff_seconds", DEFAULT_RETRY_BACKOFF_SECONDS),
            config.get("max_retry_backoff_seconds", DEFAULT_MAX_RETRY_BACKOFF_SECONDS),
        )
        return reply.strip()
    except Exception:
        return failure_detail


#: Matched against the FIRST LINE of a reply (see match_request_directive),
#: never the whole reply. The manifest asks for a bare line and nothing else,
#: but a thinking model routinely leaks a fragment of its reasoning after it
#: ("REQUEST: src/x.rs\n\n audiencia? no. Must exact shape."). Anchored to the
#: end of the WHOLE reply, those matched nothing, fell through to extract_diff
#: and ended the attempt on "no diff in model response": 345 of the 362 such
#: failures in the 2026-08-08 fleet run were a well-formed REQUEST thrown away
#: for its trailing noise, each one costing a whole attempt. Free reads do not
#: help a request that is never recognized as one in the first place.
REQUEST_RE = re.compile(r"^REQUEST:\s*(.+?)\s*$", re.IGNORECASE)

#: Reading files is FREE. A REQUEST that actually serves the fixer
#: something it has not already seen does not consume any budget, however
#: many of them it sends -- a fixer that wants to read thirty files before
#: writing a line of Rust is a fixer doing its job, and the old flat cap of
#: 20 investigation turns was rationing the cheapest thing in the loop.
#: (The three answer shapes that count as "served" are the SERVED_* prefixes
#: below; resolve_request builds every one of its successful answers from
#: them, so the two cannot drift apart.)
#:
#: What is still budgeted is an UNPRODUCTIVE request: one whose path did
#: not resolve, one whose range starts past the end of the file, or one
#: re-asking for content still visible in the conversation (a re-ask of
#: content compaction has elided is served again, free). Those
#: buy nothing, and they are the only shape a model can emit forever -- the
#: repo has a finite number of real files, so free-but-productive requests
#: terminate on their own while free-and-unproductive ones do not. The
#: budget below is therefore a wasted-turn allowance, NOT a reading limit.
DEFAULT_MAX_REQUEST_TURNS = 20  # unproductive (unresolved/repeat) REQUESTs before a diff is required
DEFAULT_MAX_REQUEST_REPEATS = 3  # identical REQUESTs before a pivot nudge replaces the content

#: Absolute ceiling on TOTAL REQUEST turns in one attempt_build, productive
#: ones included. Same role as DEFAULT_MAX_PATCH_CHUNKS: not a budget the
#: model is meant to feel, but a backstop so a pathological loop (a model
#: walking a generated directory file by file, say) cannot spend an
#: unbounded number of paid calls. Set far above any real investigation --
#: the longest genuine transcripts read on the order of 30 files.
DEFAULT_MAX_REQUEST_TURNS_CEILING = 250

DEFAULT_HEXDUMP_BYTES = 2048

PATCH_HEADER_RE = re.compile(r"^PATCH\s+(\d+)\s*/\s*(\d+)\b", re.IGNORECASE)
DEFAULT_MAX_PATCH_CHUNKS = 40  # hard safety cap, independent of the declared N -- a
# misbehaving/looping model must not be able to stall an attempt forever

#: Ceiling on how many times attempt_build will nudge a model that keeps
#: submitting a diff whose ```diff fence never closes (see
#: _reply_has_unterminated_diff_fence) instead of switching to PATCH i/N.
#: This path never applies anything (a salvaged-from-truncation diff must
#: never reach git_apply_fn -- see the check on the final-diff path), so
#: nothing else bounds it; without this ceiling a model that keeps
#: truncating even after being told to chunk could spin the while loop
#: forever. One retry is the expected case; a second covers a first
#: PATCH 1/N chunk that ALSO ran long.
DEFAULT_MAX_TRUNCATION_RETRIES = 2

VERIFY_RE = re.compile(r"^VERIFY\b", re.IGNORECASE)
DEFAULT_MAX_VERIFY_TURNS = 10   # trial-compile turns per attempt_build invocation
DEFAULT_MAX_CHECK_OUTPUT_CHARS = 3000  # tail-trim: Rust errors summarize at the end


def match_request_directive(reply):
    """The REQUEST match for `reply`, or None if it isn't one.

    Only the first line is considered, so trailing reasoning noise cannot
    void an otherwise valid request. A reply carrying a real diff is never
    read as a REQUEST -- the diff is the more advanced move and must win,
    which also means a stray leading REQUEST line can't strand one.

    VERIFY_RE and PATCH_HEADER_RE need no equivalent: neither anchors to
    the end of the reply, so both already tolerate a trailing fragment.
    """
    match = REQUEST_RE.match(reply.strip().split("\n", 1)[0])
    if match is None or extract_diff(reply) is not None:
        return None
    return match


#: Footer on every REQUEST answer that actually served something. A
#: CONSTANT string, deliberately: see rule 1 in render_request_budget_footer
#: about the prompt cache. A per-turn counter here would invalidate the
#: cached prefix on every read, which is exactly the cost we do not want to
#: attach to reading now that reading is unlimited.
REQUEST_FREE_FOOTER = (
    "(Reading is free -- REQUEST as many files as you need, there is no limit on them "
    "and they do not count against any budget. Only a REQUEST that buys nothing is "
    "charged: a path that does not resolve, a range starting past the end, or a re-ask "
    "of content still visible earlier in this conversation. If an earlier answer was "
    "elided to save space, re-REQUESTing it is free. Keep investigating until you "
    "actually understand the layout; a diff written from a guess costs far more "
    "than another read.)"
)


def render_request_budget_footer(served, wasted_used, max_wasted):
    """The one-line notice appended to the END of every REQUEST answer.

    `served` is whether the answer carried real content. A served answer
    gets REQUEST_FREE_FOOTER and is charged nothing -- reading files is
    unlimited (see DEFAULT_MAX_REQUEST_TURNS). Only an unproductive
    answer -- a path that did not resolve, a range past the end, or a
    re-ask of content still visible in the conversation -- gets a
    counter, and `wasted_used` is the count of those INCLUDING the turn
    being answered.

    Two rules encoded here, both learned the hard way:

    1. It must be a footer, never a header. Every user message in this
       conversation sits inside the region the provider's prompt cache
       reuses (see PROMPT_SECTION_ORDER for the same principle on the
       prompt side): a counter that changes every turn near the TOP would
       invalidate the cached prefix for every subsequent call in the
       attempt. At the very end it only ever invalidates itself.

    2. On the LAST allowed wasted turn it stops being a counter and becomes
       a pre-emptive instruction. The old `nudged_to_stop_investigating`
       message only arrived AFTER the budget was already spent -- in the
       RW2 transcript (2026-07-26T21:23) the model, never told what its
       budget was, spent turn 24 of 25 on another REQUEST. A model that
       cannot see the budget cannot ration it.

       The inverse now matters just as much: a model that sees a budget
       WILL ration against it. That is why a served read says "free" in so
       many words instead of staying silent -- silence reads as the old cap
       still being in force, and the fixer keeps under-investigating.
    """
    if served:
        return REQUEST_FREE_FOOTER
    remaining = max_wasted - wasted_used
    if remaining > 0:
        return (
            f"(That REQUEST bought nothing -- wasted-request {wasted_used} of {max_wasted}, "
            f"{remaining} left. Reading NEW content is still free and unlimited; what is "
            "charged is a path that does not resolve, a range past the end, or a re-ask "
            "of content still visible above. If a directory listing came back with this "
            "answer, request a real path from it; if this was a re-read, the content is "
            "already in this conversation -- reuse it from there.)"
        )
    return (
        f"(That REQUEST bought nothing -- wasted-request {wasted_used} of {max_wasted}, "
        "and this was your LAST. No wasted-request allowance remains: ANY further "
        "REQUEST will be discarded unanswered. Your next reply must be your best-effort "
        "diff -- a plan plus one ```diff block, or VERIFY plus one ```diff block -- "
        "based on what you have already seen, even if you are not fully certain.)"
    )


#: The one forced retry attempt_build allows itself when a model replies
#: REQUEST with no wasted-request allowance left. (Hitting the total
#: ceiling instead gets FORCED_DIFF_DEMAND_CEILING below -- the two paths
#: ran out of DIFFERENT things, and naming the wrong one teaches the
#: wrong lesson.) Deliberately absolute: by this point the model has
#: already been told, on its previous turn, that this exact thing would
#: happen.
#: Anything softer ("...or a REQUEST if you must") is how the transcript
#: ended up spending its last turns on investigation.
#:
#: It names WHICH budget ran out, because the honest answer is no longer
#: "you read too much" -- reading was free and unlimited. What the model
#: spent was its allowance for guessing at paths and re-reading what it
#: already had, and a message that blamed investigation in general would
#: teach exactly the wrong lesson for the next attempt.
FORCED_DIFF_DEMAND = (
    "Your wasted-request allowance is gone and that REQUEST was DISCARDED unanswered -- "
    "no file contents are coming. Reading real files was free and unlimited; what ran "
    "out was the allowance for paths that do not resolve and for re-reading what you "
    "were already shown, and you spent it. This is your final turn of this attempt: "
    "reply with a diff and nothing else. Send 2-3 sentences of plan followed by exactly "
    "ONE ```diff fenced block containing your best-effort change, however uncertain. "
    "Another REQUEST, VERIFY or PATCH reply ends the attempt with no fix at all, so a "
    "guess that might be wrong is strictly better than another question."
)


#: The ceiling variant. On this path nothing the model was promised ran
#: out -- every read may have resolved and been served free -- it hit
#: max_request_turns_ceiling, the runaway backstop on TOTAL REQUESTs.
#: Claiming the wasted-request allowance was spent here (the old, shared
#: wording) was confidently false: wasted_requests_used can be exactly 0
#: on this path, and it contradicted REQUEST_FREE_FOOTER's "no limit"
#: promise shown on every preceding served read.
FORCED_DIFF_DEMAND_CEILING = (
    "You have hit this attempt's safety ceiling on TOTAL REQUESTs, and that REQUEST was "
    "DISCARDED unanswered -- no file contents are coming. Nothing you were told was "
    "limited ran out: reading was free, but one attempt only gets so many turns of any "
    "kind before it must produce a change. This is your final turn of this attempt: "
    "reply with a diff and nothing else. Send 2-3 sentences of plan followed by exactly "
    "ONE ```diff fenced block containing your best-effort change, however uncertain. "
    "Another REQUEST, VERIFY or PATCH reply ends the attempt with no fix at all, so a "
    "guess that might be wrong is strictly better than another question."
)


#: Sent in place of ever calling git_apply_fn on a diff extract_diff
#: salvaged (via DIFF_BLOCK_UNCLOSED_RE) from a reply with an unclosed
#: ```diff fence (see _reply_has_unterminated_diff_fence) -- the reply hit
#: config["max_tokens"] mid-diff. Such content is incomplete BY
#: CONSTRUCTION and can look like a perfectly valid, complete diff if the
#: cut landed on a hunk boundary (measured: a 2-hunk diff truncated at the
#: start of hunk 2 applies cleanly as a 1-hunk diff, silently dropping the
#: rest) -- never worth the risk of applying, so this fires instead,
#: naming the real cause and pointing at the shape the manifest already
#: promised for exactly this case.
TRUNCATED_DIFF_RETRY_DEMAND = (
    "That reply cut off mid-diff -- a ```diff fence was opened but never closed, which "
    "means it hit the maximum reply length before the diff was complete. What was captured "
    "is incomplete and was NOT applied (a partial diff can look complete enough to apply "
    "cleanly while silently missing everything after the cut, which is worse than not "
    "applying at all). Resend using the PATCH i/N chunking protocol described at the top "
    "of this conversation: send everything you already had, cut at a clean point (mid-hunk "
    "is fine), as the line \"PATCH 1/N\" followed by ONE ```diff fenced chunk -- you'll be "
    "prompted for each next chunk. Do not restart the diff from scratch inside that chunk; "
    "keep exactly what you had and continue from there."
)


def hex_dump(data, max_bytes=DEFAULT_HEXDUMP_BYTES):
    """Render up to max_bytes of data as classic 16-bytes-per-line hex+ASCII,
    the way a human would inspect an unfamiliar binary segment."""
    data = data[:max_bytes]
    lines = []
    for i in range(0, len(data), 16):
        chunk = data[i:i + 16]
        hex_part = " ".join(f"{b:02x}" for b in chunk)
        ascii_part = "".join(chr(b) if 32 <= b < 127 else "." for b in chunk)
        lines.append(f"{i:08x}  {hex_part:<47}  {ascii_part}")
    return "\n".join(lines)


REQUEST_RANGE_RE = re.compile(r"^(.*?):(\d+)-(\d+)$")
#: "path:400-" -- from line 400 to end of file. The RW2 worker transcript
#: (deepseek-v4-pro, 2026-07-26T21:23) opened with exactly this form:
#: "REQUEST: src/parsers/xmp/rdf_parser.rs:400-", a perfectly sensible "show
#: me the rest of the file". The START-END-only regex above didn't match, so
#: ":400-" was treated as part of the FILENAME and the model got a
#: could-not-resolve rejection it had no way to diagnose -- it never asked
#: for a range again that attempt.
REQUEST_OPEN_END_RANGE_RE = re.compile(r"^(.*?):(\d+)-$")
#: "path:-120" -- start of file through line 120.
REQUEST_OPEN_START_RANGE_RE = re.compile(r"^(.*?):-(\d+)$")
#: "path:400" -- a bare line number, the shape a model produces when it's
#: quoting a line from a compiler error or a grep hit.
REQUEST_SINGLE_LINE_RE = re.compile(r"^(.*?):(\d+)$")

#: Window served for a bare "path:N". Weighted forward rather than centered:
#: a bare line number nearly always comes from an error/grep pointing at the
#: START of something the model wants to read (a fn, a match arm), so it
#: needs the body after N more than the code before it. The few lines before
#: are there for the signature/attribute line above the hit.
REQUEST_SINGLE_LINE_CONTEXT_BEFORE = 20
REQUEST_SINGLE_LINE_CONTEXT_AFTER = 100


def parse_request_range(path_str):
    """Split a "path:RANGE" request into (path, start, end).

    Accepted range shapes (all 1-indexed, all inclusive):
      "path:40-120"  -> (path, 40, 120)
      "path:400-"    -> (path, 400, None)  -- None end means "to EOF"
      "path:-120"    -> (path, 1, 120)
      "path:400"     -> a window around/after 400 (see the two
                        REQUEST_SINGLE_LINE_CONTEXT_* constants)

    Returns (path, None, None) when there's no numeric range suffix. A
    range-shaped suffix with start < 1 or start > end strips the suffix
    but returns no range -- whole-file fallback -- rather than failing
    the entire request over a typo'd range. A non-numeric suffix (e.g.
    "x.rs:a-b") isn't range-shaped at all, so it stays part of the path
    and fails resolution with the normal could-not-resolve message.
    """
    stripped = path_str.strip()

    m = REQUEST_RANGE_RE.match(stripped)
    if m:
        start, end = int(m.group(2)), int(m.group(3))
        if start < 1 or end < start:
            return m.group(1), None, None
        return m.group(1), start, end

    m = REQUEST_OPEN_END_RANGE_RE.match(stripped)
    if m:
        start = int(m.group(2))
        if start < 1:
            return m.group(1), None, None
        return m.group(1), start, None

    m = REQUEST_OPEN_START_RANGE_RE.match(stripped)
    if m:
        end = int(m.group(2))
        if end < 1:
            return m.group(1), None, None
        return m.group(1), 1, end

    m = REQUEST_SINGLE_LINE_RE.match(stripped)
    if m:
        line = int(m.group(2))
        if line < 1:
            return m.group(1), None, None
        return (
            m.group(1),
            max(1, line - REQUEST_SINGLE_LINE_CONTEXT_BEFORE),
            line + REQUEST_SINGLE_LINE_CONTEXT_AFTER,
        )

    return stripped, None, None


#: How many sibling names a could-not-resolve rejection lists. 40 short Rust
#: filenames is a few hundred tokens -- cheap next to the turn (~17K tokens
#: of conversation, one model call) that a blind retry costs.
REQUEST_DIR_LISTING_LIMIT = 40


def describe_missing_path(path_part, repo_root, samples_dir, max_entries=REQUEST_DIR_LISTING_LIMIT):
    """Build the "here is what's ACTUALLY there" half of a could-not-resolve
    rejection: the real entries of the nearest existing ancestor directory of
    the path the model asked for, plus a did-you-mean when one of them is a
    near miss on the name it invented.

    Why: in the RW2 transcript (2026-07-26T21:23) the model asked for
    src/parsers/xmp/artwork_parser.rs -- a plausible name that does not
    exist; the directory really holds history_parser.rs, mod.rs,
    namespace_mapping.rs, namespace_resolver.rs, namespaces/ and
    rdf_parser.rs. The old rejection said "try a path from the list shown",
    but by then the prompt's file list was thousands of tokens back in a
    ~17K-token conversation, so "the list shown" pointed at nothing the
    model could still see, and it burned the next turn guessing again.

    Returns "" when nothing useful can be said (no ancestor inside either
    root), so the caller can fall back to the bare rejection.
    """
    requested = Path(path_part)
    lines = []
    for root, label in (
        (repo_root, "repo root"),
        (Path(samples_dir) if samples_dir is not None else None, "samples dir"),
    ):
        if root is None:
            continue
        try:
            root_resolved = Path(root).resolve()
            target = (root_resolved / requested).resolve()
        except (OSError, ValueError):
            # ValueError: embedded NUL in the model-supplied path -- same
            # guard as resolve_request's candidate loop.
            continue
        # Containment is checked on the RESOLVED target, never on the
        # lexical one: "../../../etc/passwd" under root has root itself in
        # its lexical .parents chain, so a lexical check would happily list
        # /etc for an escape attempt. resolve_request's own candidate loop
        # refuses to SERVE such a path; this must not describe it either.
        if root_resolved != target and root_resolved not in target.parents:
            continue
        # Walk up from the requested path's own parent to the nearest
        # ancestor that exists -- a typo'd directory ("src/parsr/x.rs") is
        # just as common as a typo'd filename, and listing the grandparent
        # is still far better than listing nothing.
        for ancestor in target.parents:
            if root_resolved != ancestor and root_resolved not in ancestor.parents:
                continue  # past the top of this root
            if not ancestor.is_dir():
                continue
            try:
                entries = sorted(
                    entry.name + ("/" if entry.is_dir() else "")
                    for entry in ancestor.iterdir()
                    if not entry.name.startswith(".")
                )
            except OSError:
                break
            rel = ancestor.relative_to(root_resolved)
            shown = entries[:max_entries]
            suffix = ""
            if len(entries) > max_entries:
                suffix = f" ... (+{len(entries) - max_entries} more)"
            where = f"{rel}/" if str(rel) != "." else f"the {label}"
            lines.append(f"{where} actually contains: {', '.join(shown)}{suffix}" if shown
                         else f"{where} is empty.")
            close = difflib.get_close_matches(
                requested.name, [entry.rstrip("/") for entry in entries], n=3, cutoff=0.6
            )
            if close:
                prefix = f"{rel}/" if str(rel) != "." else ""
                lines.append("Did you mean: " + ", ".join(f"{prefix}{name}" for name in close) + "?")
            break
    return "\n".join(lines)


#: The three shapes resolve_request uses for an answer that actually
#: carried content. attempt_build charges a REQUEST turn against the
#: wasted-request budget unless the answer starts with one of these, so
#: every successful return below is built from one of these constants
#: rather than an inline literal -- if a fourth success shape is ever
#: added it must be added here too, or reads of it start costing budget.
#:
#: A whitelist, not a blacklist of failures, and deliberately so: an
#: answer shape nobody anticipated is charged rather than waved through,
#: which is the side that stays bounded.
SERVED_HEXDUMP_PREFIX = "Hex dump of "
SERVED_LINES_PREFIX = "Lines "
SERVED_CONTENTS_PREFIX = "Contents of "
_SERVED_PREFIXES = (SERVED_HEXDUMP_PREFIX, SERVED_LINES_PREFIX, SERVED_CONTENTS_PREFIX)


def request_answer_served(body):
    """True when a resolve_request answer carried real file content, i.e.
    when the REQUEST that produced it was productive and must therefore be
    free (see DEFAULT_MAX_REQUEST_TURNS). False for a could-not-resolve
    rejection or a past-the-end range, both of which buy the fixer
    nothing and are what the wasted-request budget exists to bound."""
    return str(body).startswith(_SERVED_PREFIXES)


def resolve_request(path_str, repo_root, samples_dir, max_text_bytes=20_000):
    """Answer a model's "REQUEST: <path>" turn -- a hex dump if the path
    resolves under samples_dir (real binary sample data), the raw text if
    it resolves under repo_root (more source to read), or a rejection
    message otherwise. Path traversal outside both roots is refused.
    A range suffix on a source file returns just that 1-indexed inclusive
    line range, numbered -- see parse_request_range for the four accepted
    shapes ("path:40-120", "path:400-", "path:-120", "path:400"). Samples
    always get the whole-file hex dump.

    The rejection is deliberately self-correcting rather than a bare "no":
    it lists the real contents of the nearest existing ancestor directory
    (see describe_missing_path), because a model that guessed a filename
    cannot un-guess it from a message that shows it nothing.
    """
    path_part, range_start, range_end = parse_request_range(path_str)
    candidates = []
    if samples_dir is not None:
        candidates.append((Path(samples_dir) / path_part, "sample"))
    candidates.append((repo_root / path_part, "source"))

    for candidate, kind in candidates:
        try:
            resolved = candidate.resolve()
        except (OSError, ValueError):
            # ValueError: an embedded NUL in a model-supplied path.
            # Path.resolve raises it as ValueError, not OSError, and
            # nothing above attempt_build catches either -- without this
            # a single "REQUEST: src/\x00.rs" reply kills the worker.
            continue
        root = (Path(samples_dir).resolve() if kind == "sample" else repo_root.resolve())
        if root not in resolved.parents and resolved != root:
            continue
        if not resolved.is_file():
            continue
        if kind == "sample":
            data = resolved.read_bytes()
            return (
                f"{SERVED_HEXDUMP_PREFIX}{path_part} ({len(data)} bytes total, "
                f"showing first {min(len(data), DEFAULT_HEXDUMP_BYTES)}):\n"
                f"{hex_dump(data)}"
            )
        content = resolved.read_text(errors="replace")
        if range_start is not None:
            lines = content.splitlines()
            if range_start > len(lines):
                asked = f"{range_start}-{range_end}" if range_end is not None else f"{range_start}-EOF"
                # Starts with a CONSTANT, never with path_part: this answer
                # is unproductive and must fail the SERVED_* whitelist, but
                # path_part is model-controlled, and a resolvable path
                # spelled to begin with "Lines " or "Contents of " (e.g.
                # "Lines /../src/x.rs") would smuggle it past
                # request_answer_served at character 0.
                return (
                    f"Requested range {asked} starts past the end -- {path_part} has "
                    f"only {len(lines)} lines. Request a range within the file."
                )
            # range_end None is the "path:400-" open-ended form: everything
            # from start to EOF.
            clamped_end = len(lines) if range_end is None else min(range_end, len(lines))
            numbered = "\n".join(
                f"{i}: {line}"
                for i, line in enumerate(lines[range_start - 1:clamped_end], start=range_start)
            )
            # Same cap as the whole-file branch below. Without it a single
            # open-ended range on a generated file is a multi-megabyte
            # "free" read (src/exiftool_tables/ files run to 5MB+) that
            # blows the provider context window for every later call in
            # the attempt -- and an over-context 400 is an uncharged infra
            # failure, so the loop would grind on it indefinitely.
            if len(numbered) > max_text_bytes:
                kept = numbered[:max_text_bytes]
                if "\n" in kept:
                    kept = kept[:kept.rfind("\n")]
                numbered = (
                    f"{kept}\n[... truncated at {max_text_bytes} characters -- "
                    "narrow the range to see the rest]"
                )
            return f"{SERVED_LINES_PREFIX}{range_start}-{clamped_end} of {path_part}:\n{numbered}"
        if len(content) > max_text_bytes:
            # Say so out loud: the charging story ("re-asking for content
            # still visible") only stays honest if the model can tell it
            # was NOT shown the whole file. A ranged read of the rest is
            # new content and stays free.
            return (
                f"{SERVED_CONTENTS_PREFIX}{path_part}:\n{content[:max_text_bytes]}"
                f"\n[... truncated at {max_text_bytes} of {len(content)} characters -- "
                "request a line range for the rest]"
            )
        return f"{SERVED_CONTENTS_PREFIX}{path_part}:\n{content}"

    rejection = f"Could not resolve {path_part!r} under the samples dir or repo root."
    listing = describe_missing_path(path_part, repo_root, samples_dir)
    if listing:
        return f"{rejection}\n{listing}"
    return f"{rejection} Try a path from the list shown earlier in this conversation."


# --- Rejection taxonomy ------------------------------------------------------
#
# Every failed attempt used to reach the worker as one of two strings:
# "no working fix after repair attempt" (which conflated "the patch never
# applied" with "it applied and the compiler rejected it") and "gap count
# did not decrease" (which conflated "your patch was wrong", "your patch
# closed A and the rebuild revealed B", and "no diff to this file can EVER
# move this number because the code path is unreachable"). A worker given
# either string has nothing to act on, so it resends a variation of the
# same doomed approach until its fail budget runs out. 419 successful
# model calls in one hour landed zero tags on 2026-07-30, and every gap
# closed by hand that day fell into a class no worker could have
# diagnosed from those two strings.
#
# These codes are carried INSIDE the reason text (see annotate_rejection)
# rather than in a new field, deliberately: the reason is what already
# flows to every consumer -- the repair turn's "That attempt failed"
# message, run_tag_loop's persisted per-tag attempts list (and from there
# format_previous_attempts, i.e. the NEXT prompt for this tag), the K1
# lessons ledger, and watch_parallel_fix.py's log scraper. Adding a field
# would have reached none of them without touching all of them.

#: The model never produced a diff that git apply accepted.
REJECT_PATCH_NOT_APPLIED = "patch-did-not-apply"
#: The diff applied cleanly; cargo rejected it.
REJECT_BUILD_FAILED = "build-failed"
#: Built, but the tag this attempt targeted is still missing from oxidex.
REJECT_TAG_STILL_ABSENT = "tag-still-absent"
#: Built, the tag now appears, but its value disagrees with ExifTool.
REJECT_WRONG_VALUE = "wrong-value"
#: Built, and the format's gap SET is byte-identical before and after --
#: the patch changed nothing observable. This is the structural signal:
#: the target is out of reach of a single-tag diff to this parser.
REJECT_GAP_SET_UNCHANGED = "gap-set-unchanged"
#: Built, the gap COUNT did not move, but the gap SET did -- the patch
#: closed one gap and the rebuild revealed another. Emphatically not the
#: same failure as the one above, and the count alone cannot tell them
#: apart (see gap_set_delta).
REJECT_GAP_SET_CHURNED = "gap-set-churned"
#: Not a patch failure at all: oxidex emits nothing format-specific for
#: this format's own sample, so its parser never runs and no diff to it
#: can move the gap. Raised BEFORE a worker is dispatched.
REJECT_FORMAT_UNREACHABLE = "format-unreachable"

REJECTION_MARKER_RE = re.compile(r"\[reject:([a-z-]+)\]")


def annotate_rejection(reason, code, detail=""):
    """Stamp a rejection code (and optional actionable detail) onto a
    failure reason, idempotently.

    The marker is APPENDED, never prepended and never a replacement: every
    existing consumer that greps the leading text of a reason
    (watch_parallel_fix.REGRESSED_RE's "gap count did not decrease",
    distill_lessons.INFRA_REASON_RE's "model call failed:", the tests that
    assert on the historical strings) keeps working unchanged, while the
    code and its explanation ride along for the worker and the distiller.

    A reason that already carries a marker is returned untouched -- the
    first (most specific) classification wins, so a reason annotated deep
    in fix_gap is not relabelled by a broader classifier further out.
    """
    reason = str(reason or "")
    if not code or REJECTION_MARKER_RE.search(reason):
        return reason
    marker = f"[reject:{code}]"
    return f"{reason} {marker} {detail}".rstrip() if detail else f"{reason} {marker}"


def rejection_code(reason):
    """The rejection code carried by a reason string, or None. Readers
    (format_previous_attempts, dashboards, the distiller) use this rather
    than re-deriving a classification from prose."""
    m = REJECTION_MARKER_RE.search(str(reason or ""))
    return m.group(1) if m else None


def gap_keys(report):
    """The SET of still-open gap keys in one per-format comparison dict --
    missing_in_oxidex ("family:name") and value_differences ("tag_key")
    together, since a tag that moves between those two lists has not been
    closed (see tag_still_open's own docstring for how that exact confusion
    shipped a wrong-valued XMP:ArtworkTitle).

    A missing/falsy report is an empty set, not an error: callers compare
    two of these and an unavailable side must degrade to "cannot tell",
    which they check for explicitly rather than inferring from {}.
    """
    report = report or {}
    keys = {
        f"{e.get('family')}:{e.get('name')}"
        for e in (report.get("missing_tags") or report.get("missing_in_oxidex") or [])
    }
    keys |= {
        str(d.get("tag_key"))
        for d in (report.get("value_differences") or [])
        if d.get("tag_key")
    }
    return keys


def gap_set_delta(pre_report, post_report):
    """Compare the SET of open gaps before and after an attempt, not the
    count. Returns {"closed": [...], "opened": [...], "still_open": [...]},
    each sorted.

    gap_count is a single integer, and a patch that closes gap A while the
    rebuild reveals gap B leaves that integer untouched -- indistinguishable
    from a patch that did nothing at all, which is how a worker that was
    genuinely making progress got told "gap count did not decrease" and
    burned its remaining rounds re-deriving the same change. This project
    has been bitten by count-vs-set comparisons repeatedly; the count is
    kept as the gate (it is what the commit's Verified: trailer promises)
    but the SET is what the worker is told about.

    Pure set arithmetic over two comparison dicts. Lineage -- that the two
    reports describe the same format in the same worktree modulo the change
    under test -- is the caller's responsibility, exactly as for
    new_oxidex_only_keys.
    """
    pre, post = gap_keys(pre_report), gap_keys(post_report)
    return {
        "closed": sorted(pre - post),
        "opened": sorted(post - pre),
        "still_open": sorted(pre & post),
    }


#: A rendered gap-set delta names at most this many tags per bucket; the
#: rest are summarized as a count. A NEF rebuild can reveal dozens at once
#: and the reason string rides in a 2000-byte K1 ledger line.
GAP_SET_DELTA_MAX_NAMED = 6


def _name_list(keys, limit=GAP_SET_DELTA_MAX_NAMED):
    shown = ", ".join(keys[:limit])
    if len(keys) > limit:
        shown += f", +{len(keys) - limit} more"
    return shown


def format_gap_set_delta(delta):
    """Render gap_set_delta's dict as one human/model-readable clause,
    e.g. "closed 1 (EXIF:Make), opened 2 (EXIF:Model, EXIF:Software)".
    "" for a None delta so callers can concatenate unconditionally."""
    if not delta:
        return ""
    parts = []
    for bucket, label in (("closed", "closed"), ("opened", "opened")):
        keys = delta.get(bucket) or []
        if keys:
            parts.append(f"{label} {len(keys)} ({_name_list(keys)})")
    if not parts:
        return f"gap set unchanged ({len(delta.get('still_open') or [])} still open)"
    return ", ".join(parts)


#: Appended to a REJECT_GAP_SET_UNCHANGED reason. This is the one message
#: whose whole job is to stop the worker resending a variation of the same
#: patch: nothing it did was observable, so the fault is upstream of the
#: lines it edited.
GAP_SET_UNCHANGED_ADVICE = (
    "The gap set is IDENTICAL before and after your patch -- not one tag closed, "
    "not one opened. Your change had no observable effect, so the problem is not "
    "the lines you edited. Before patching this file again, prove the code you are "
    "editing actually runs for this sample: check that the format is detected at "
    "all (a signature declared past the detection buffer resolves the file to "
    "Unknown and the parser never runs), that there is only ONE live parser for "
    "this format (a second, dead copy absorbs patches silently), and that the "
    "values the parser needs (byte order, base offsets) are actually threaded to "
    "the function you changed. If none of those hold, this target cannot be closed "
    "by a diff to this file and should be reported rather than retried."
)


def classify_build_rejection(reason):
    """The rejection code for a reason produced by attempt_build's
    `built is False` path, without re-parsing prose.

    Returns the code already stamped on the reason when there is one (the
    apply-exhaustion path stamps REJECT_PATCH_NOT_APPLIED itself), None for
    an infrastructure failure (a 429 is not a rejection of anything -- see
    INFRA_FAILURE_PREFIX and run_tag_loop's infra_only branch, which
    charges nothing for it), and REJECT_BUILD_FAILED otherwise. "Otherwise"
    is deliberately the fallback rather than a positive match: every
    remaining way out of attempt_build with built=False (a compile error,
    "no diff in model response", an exhausted request/verify budget) is a
    failure of the model's own output that a compiler or a parser already
    described in the conversation."""
    reason = str(reason or "")
    if reason.startswith(INFRA_FAILURE_PREFIX):
        return None
    return rejection_code(reason) or REJECT_BUILD_FAILED


def classify_recheck_rejection(base_reason, pre_report, post_report, tag_verdicts=None):
    """Turn a failed recheck into (annotated_reason, code, delta).

    `base_reason` is whatever fix_gap already computed ("gap count did not
    decrease", or recheck_fn's own detail string) and is preserved verbatim
    at the head of the result.

    Precedence, most actionable first, and every verdict below is backed
    by evidence rather than inferred from the count:
      wrong-value      -- the target tag is present now but disagrees with
                          ExifTool; the worker has a concrete value to chase.
      gap-set-unchanged-- not one tag opened or closed anywhere in this
                          format. Structurally out of reach.
      tag-still-absent -- the set DID move, and the target is still open.
                          The patch did something; it did not do this.
      gap-set-churned  -- the set moved and the target is no longer open,
                          so the count is only flat because the rebuild
                          revealed as many gaps as this patch closed. The
                          approach WORKS and must not be thrown away.

    pre_report/post_report None is the legacy int/2-tuple recheck_fn
    contract: no set information exists, so nothing can honestly be
    claimed beyond what the count already said. The base reason is
    returned UNANNOTATED (with REJECT_TAG_STILL_ABSENT reported to the
    caller for bookkeeping) -- a marker there would assert evidence this
    function does not have, and would change a string that predates it.

    `tag_verdicts` is the list of tag_still_open() results for this gap's
    own target tags, so this function stays pure -- fix_gap already
    computes them.
    """
    for verdict in tag_verdicts or []:
        if verdict and verdict[0] == "value_differs":
            return annotate_rejection(base_reason, REJECT_WRONG_VALUE), REJECT_WRONG_VALUE, None
    if pre_report is None or post_report is None:
        return str(base_reason), REJECT_TAG_STILL_ABSENT, None
    delta = gap_set_delta(pre_report, post_report)
    if not delta["closed"] and not delta["opened"]:
        return (
            annotate_rejection(base_reason, REJECT_GAP_SET_UNCHANGED, GAP_SET_UNCHANGED_ADVICE),
            REJECT_GAP_SET_UNCHANGED,
            delta,
        )
    moved = f"the gap COUNT is flat but the gap SET moved: {format_gap_set_delta(delta)}."
    if any(tag_verdicts or []):
        return (
            annotate_rejection(
                base_reason, REJECT_TAG_STILL_ABSENT,
                f"{moved} So your patch DID have an effect -- it just was not on the tag "
                "you were asked to close, which is still open. Keep what worked and aim "
                "it at the target.",
            ),
            REJECT_TAG_STILL_ABSENT,
            delta,
        )
    return (
        annotate_rejection(
            base_reason, REJECT_GAP_SET_CHURNED,
            f"{moved} Your patch closed its target; the rebuild revealed as many gaps as "
            "it closed, which is why the count did not drop. Do not discard this "
            "approach -- extend it to the newly-opened tag(s).",
        ),
        REJECT_GAP_SET_CHURNED,
        delta,
    )


def attempt_build(messages, *, call_model_fn, git_apply_fn, git_checkout_clean_fn,
                   cargo_build_fn, config, repo_root, pick_model_fn=random.choice,
                   samples_dir=None, cargo_check_fn=None):
    """Try to get a working build via a conversation with UNLIMITED
    reading: the model may send as many REQUEST: <path> turns as it likes
    (see resolve_request) and none of them cost it anything, then up to 2
    diff attempts (initial + one apply/build repair round-trip).

    What is bounded is unproductive investigation, not investigation.
    config["max_request_turns"] is a wasted-request allowance, spent only
    by a REQUEST whose path did not resolve, whose range starts past the
    end, or whose answer's payload is still visible in the conversation
    (see request_answer_served and the visibility check below -- content
    compaction has elided is re-served free); a productive read is free
    and is told so, in words, by REQUEST_FREE_FOOTER -- silence would
    leave the model rationing against a cap that no longer exists.
    Every REQUEST answer ends with that footer or with the wasted-request
    counter (render_request_budget_footer), the last one pre-emptively
    demanding a diff; a REQUEST sent after the allowance is gone buys
    exactly one forced-diff retry (FORCED_DIFF_DEMAND) and never more.
    config["max_request_turns_ceiling"] is the runaway backstop on TOTAL
    requests -- reached only by a loop, and named to the model only at
    the moment it fires (FORCED_DIFF_DEMAND_CEILING).
    Extends the given messages conversation in place. Returns
    (built, reason, diff, messages) -- reason is None when built is True;
    diff is the successfully-applied diff (None if not built).

    pick_model_fn(models) -> model_spec is called fresh before every
    individual model call (not once per attempt_build invocation), so a
    repair round-trip can land on a different model -- potentially a
    different provider entirely -- from config["models"] than the initial
    attempt. Each spec is a {"name", "base_url", "api_key"} dict.

    A single reply is capped at config["max_tokens"] (see build_prompt's
    separate config["max_prompt_tokens"] on the request side -- the two are
    independent budgets), so a large diff may not fit in one turn.
    build_reply_shape_manifest tells the model to split such a diff into
    "PATCH i/N" chunks instead of truncating it silently -- see
    PATCH_HEADER_RE below. Each chunk is accumulated (up to
    DEFAULT_MAX_PATCH_CHUNKS turns, a safety ceiling independent of N, in
    case of a misbehaving/looping model) and, once chunk N/N arrives with
    every chunk 1..N present, concatenated back into one diff and applied
    exactly like a normal single-reply diff -- this doesn't consume a
    separate diff_attempts_used slot per chunk, only once the full diff is
    assembled and ready to apply.

    A model that ignores that instruction and submits a plain (non-PATCH)
    diff anyway can still get truncated mid-reply by the same cap: the
    reply's ```diff fence opens and never closes. extract_diff's
    DIFF_BLOCK_UNCLOSED_RE fallback recovers what content there is, but a
    diff cut off by a hard length cap is incomplete BY CONSTRUCTION -- and
    that incomplete content can look like a perfectly valid, COMPLETE diff
    if the cut happened to land on a hunk boundary (measured directly: a
    2-hunk diff truncated at the start of hunk 2 applies cleanly as a
    1-hunk diff, silently dropping the rest -- worse than an outright
    apply failure, since nothing downstream can tell). So a diff salvaged
    from a reply matching _reply_has_unterminated_diff_fence(reply) is
    NEVER handed to git_apply_fn: TRUNCATED_DIFF_RETRY_DEMAND fires
    instead, bounded by DEFAULT_MAX_TRUNCATION_RETRIES turns (free of
    diff_attempts_used, since nothing was ever applied) before the attempt
    fails outright rather than falling through to try the salvage anyway.
    The same guard applies to a truncated PATCH i/N chunk and a truncated
    VERIFY trial diff -- see the matching checks at PATCH_HEADER_RE and
    VERIFY_RE below.

    cargo_check_fn(repo_root) -> (success, output), if provided, enables
    the VERIFY protocol: a reply of "VERIFY" plus one ```diff fenced
    block gets that diff applied, cargo-checked, REVERTED, and the
    tail-trimmed check output fed back -- a trial compile that never
    consumes one of the 2 real diff attempts. Bounded by
    config["max_verify_turns"] (default DEFAULT_MAX_VERIFY_TURNS).
    None (the default) keeps VERIFY off: such replies get an
    "unavailable" message, so old callers and tests are unaffected.

    Exhausting both diff attempts returns "no working fix after repair
    attempt". When the LAST of those attempts died at `git apply` rather
    than at `cargo build`, the reason additionally carries
    REJECT_PATCH_NOT_APPLIED and the apply error (see annotate_rejection):
    those two exhaustion paths used to produce the identical string, so a
    worker whose diffs never even landed on disk was told the same thing
    as one whose code did not compile -- and the two need opposite fixes.
    The build-exhaustion string is left exactly as it always was; the
    compiler's own output is already in the conversation, and
    classify_build_rejection maps the unmarked string to
    REJECT_BUILD_FAILED for readers that want a code for it.
    """
    # Reading is free: `max_request_turns` bounds only the UNPRODUCTIVE
    # REQUESTs (unresolved path, or a repeat of one already served), never
    # the productive ones. The config key keeps its old name so existing
    # config.toml files keep working; what changed is what it counts.
    max_wasted_requests = config.get("max_request_turns", DEFAULT_MAX_REQUEST_TURNS)
    wasted_requests_used = 0
    # Total REQUESTs including the free ones -- only ever checked against
    # the runaway-loop ceiling, never shown to the model.
    request_turns_used = 0
    max_request_turns_ceiling = config.get(
        "max_request_turns_ceiling", DEFAULT_MAX_REQUEST_TURNS_CEILING)
    max_request_repeats = config.get("max_request_repeats", DEFAULT_MAX_REQUEST_REPEATS)
    request_counts = {}
    max_verify_turns = config.get("max_verify_turns", DEFAULT_MAX_VERIFY_TURNS)
    verify_turns_used = 0
    verify_rejections = 0
    diff_attempts_used = 0
    forced_diff_retry_used = False
    patch_chunks = {}
    patch_turns_used = 0
    # A reply whose ```diff fence never closed hit config["max_tokens"]
    # mid-diff rather than never attempting one -- worth a targeted nudge
    # toward PATCH i/N (see the check on the final-diff path below, which
    # never lets a diff salvaged from such a reply reach git_apply_fn).
    # Bounded independently of diff_attempts_used: this path never applies
    # anything, so diff_attempts_used never advances on it, and without its
    # own ceiling a model that keeps truncating even after being told to
    # chunk could spin the while loop forever.
    truncation_retries_used = 0
    # None until a real diff attempt fails; then ("apply", msg) or
    # ("build", msg) -- the only place in this module that can still tell
    # those two apart once the loop is over (see the return below).
    last_diff_failure = None
    current_phase = "explore" if len(messages) == 1 else "patch"
    while diff_attempts_used < 2:  # one initial attempt + one repair round-trip
        messages[:] = compact_messages(
            messages,
            trigger_tokens=config.get("compaction_trigger_tokens", DEFAULT_COMPACTION_TRIGGER_TOKENS),
            keep_recent=config.get("compaction_keep_recent_turns", DEFAULT_COMPACTION_KEEP_RECENT_TURNS),
            min_elide_tokens=config.get("compaction_min_elide_tokens", DEFAULT_COMPACTION_MIN_ELIDE_TOKENS),
        )
        model_spec = pick_model_fn(models_for_phase(config["models"], current_phase))
        try:
            reply = call_model_fn(
                messages, model_spec["base_url"], model_spec["api_key"], model_spec["name"],
                config["max_tokens"], model_spec.get("reasoning_effort") or config["reasoning_effort"],
                config.get("stream", False), config.get("thinking", True),
                config.get("temperature", 0), config.get("timeout", 120),
                config.get("max_retries", DEFAULT_MAX_RETRIES),
                config.get("retry_backoff_seconds", DEFAULT_RETRY_BACKOFF_SECONDS),
                config.get("max_retry_backoff_seconds", DEFAULT_MAX_RETRY_BACKOFF_SECONDS),
            )
        except Exception as e:
            # Deliberately catches ModelQuotaExhausted too (operator
            # decision 2026-07-25: empty 200s, 429s and everything else in
            # this class are things the fleet just continues past). It
            # becomes an INFRA_FAILURE_PREFIX reason, and run_tag_loop's
            # infra_only branch then charges NOTHING for it: no fail
            # increment, no attempt history, no blacklist check. Letting
            # it propagate instead would kill the worker outright, which
            # is the opposite of continuing on.
            # Network/timeout/HTTP/malformed-response failures are a normal
            # cost of "any model" -- a single bad call must not kill the
            # whole loop. No repair round-trip here: retrying the same
            # oversized/slow request immediately is unlikely to help; the
            # cross-round 2-strikes skip-list is what handles this format
            # long-term if it keeps failing.
            return False, f"model call failed: {e}", None, messages

        messages.append({"role": "assistant", "content": reply})

        request_match = match_request_directive(reply)
        if request_match:
            normalized = request_match.group(1).strip()
            request_counts[normalized] = request_counts.get(normalized, 0) + 1
            if (wasted_requests_used < max_wasted_requests
                    and request_turns_used < max_request_turns_ceiling):
                request_turns_used += 1
                body = resolve_request(request_match.group(1), repo_root, samples_dir)
                # Free means it bought NEW content, and "new" is judged on
                # the ANSWER, not the request string: a clamped range, a
                # ./-prefixed path or a re-ranged sample (ranges on samples
                # are ignored) all reach byte-identical payloads through
                # distinct strings, and each used to look "new" and be free
                # forever -- the wasted allowance never bound, and the
                # 250-REQUEST ceiling became the real per-attempt cost. The
                # question that matters is whether this exact payload is
                # still VISIBLE in the conversation. That also makes
                # compaction self-consistent: the elision stub says
                # "Re-REQUEST it if still needed", and once the payload is
                # stubbed out it is no longer visible, so obeying that
                # instruction is free instead of charged.
                whitelisted = request_answer_served(body)
                payload = body.partition("\n")[2]
                if whitelisted and payload.strip():
                    already_shown = any(
                        payload in m["content"]
                        for m in messages if m["role"] == "user"
                    )
                else:
                    # Rejections and payload-less answers (an empty file)
                    # fall back to the request-string rule.
                    already_shown = request_counts[normalized] > 1
                # A read that would serve content NOT currently visible is
                # never pivoted away -- either it is genuinely new, or the
                # compaction stub itself told the model to re-REQUEST it.
                fresh_serve = whitelisted and bool(payload.strip()) and not already_shown
                if request_counts[normalized] >= max_request_repeats and not fresh_serve:
                    # Dead-end: the same request over and over buying
                    # nothing new. Re-serving identical content burns
                    # budget without advancing anything -- course-correct
                    # instead. Two honest wordings: "provided in full" is
                    # only true when the content was actually served.
                    if whitelisted:
                        body = (
                            f"You've now requested {normalized!r} {request_counts[normalized]} times -- "
                            "it was already provided in full and re-reading it will not change anything. "
                            "Pivot: request a DIFFERENT file, narrow to a line range "
                            "(REQUEST: path:START-END), or submit your best diff now."
                        )
                    else:
                        body = (
                            f"You've now requested {normalized!r} {request_counts[normalized]} times and it "
                            "has never returned content -- the path does not resolve (or the range starts "
                            "past the end), and asking again will not change that. Pivot: request a "
                            "DIFFERENT file, pick a path from the directory listing shown earlier, or "
                            "submit your best diff now."
                        )
                    served = False
                    current_phase = "patch"
                else:
                    served = whitelisted and not already_shown
                    current_phase = "explore"
                if not served:
                    wasted_requests_used += 1
                # Footer on BOTH branches, and always LAST in the message --
                # see render_request_budget_footer for why it's a footer, why
                # a served read gets a constant string, and why the last
                # wasted turn's wording is pre-emptive.
                footer = render_request_budget_footer(
                    served, wasted_requests_used, max_wasted_requests)
                messages.append({"role": "user", "content": f"{body}\n\n{footer}"})
                continue
            if not forced_diff_retry_used:
                # Previously fell straight through to extract_diff on this
                # same REQUEST-shaped reply and failed immediately with "no
                # diff in model response" -- silently wasting the whole
                # attempt on investigation without ever telling the model to
                # actually submit something.
                #
                # This is now one FORCED retry rather than a suggestion: the
                # model was already warned, on the turn before, that a
                # REQUEST here would be discarded, so the only useful thing
                # left to say is "a diff, nothing else" (FORCED_DIFF_DEMAND).
                # Strictly one: the flag is never cleared, so a model that
                # requests yet again falls through to the return below
                # instead of earning itself unbounded extra calls.
                forced_diff_retry_used = True
                # Name the budget that actually ran out: the wasted-request
                # allowance if it is spent, otherwise the total-REQUEST
                # ceiling. (When both are gone the allowance story is still
                # true, so it wins.)
                demand = (
                    FORCED_DIFF_DEMAND
                    if wasted_requests_used >= max_wasted_requests
                    else FORCED_DIFF_DEMAND_CEILING
                )
                messages.append({"role": "user", "content": demand})
                current_phase = "patch"
                continue
            # Two very different situations wearing one string until now.
            # The budget case is a model guessing at paths; the ceiling case
            # is a model reading real files without end -- the second is a
            # loop worth seeing in the lessons ledger as its own thing,
            # since no amount of raising max_request_turns would change it.
            # Both keep the DIFF_FORMAT_FAILURE_RE-matched prefix.
            # Mirror the demand selection above: when BOTH ran out on the
            # same turn, the allowance story is the true one, for the
            # ledger reason exactly as for the model-facing message.
            if wasted_requests_used < max_wasted_requests:
                return (
                    False,
                    "no diff in model response (hit the "
                    f"{max_request_turns_ceiling}-REQUEST safety ceiling)",
                    None, messages,
                )
            return False, "no diff in model response (exhausted request budget)", None, messages

        if VERIFY_RE.match(reply.strip()):
            if cargo_check_fn is None or verify_turns_used >= max_verify_turns:
                verify_rejections += 1
                if verify_rejections >= 2:
                    return (
                        False, "no diff in model response (exhausted verify budget)",
                        None, messages,
                    )
                detail = (
                    "VERIFY is unavailable in this run"
                    if cargo_check_fn is None
                    else f"VERIFY budget ({max_verify_turns}) exhausted"
                )
                messages.append({
                    "role": "user",
                    "content": f"{detail} -- submit your final diff now (or a REQUEST if you must).",
                })
                current_phase = "explore"
                continue
            verify_turns_used += 1
            # Checked BEFORE extract_diff: DIFF_BLOCK_UNCLOSED_RE (see
            # extract_diff) can recover something from an unclosed fence,
            # but content cut off by config["max_tokens"] is incomplete by
            # construction and must never reach git_apply_fn -- see the
            # matching check on the final-diff path below for why.
            if _reply_has_unterminated_diff_fence(reply):
                messages.append({
                    "role": "user",
                    "content": (
                        "That VERIFY reply cut off mid-diff -- its ```diff fence never closed, "
                        "so the trial diff is incomplete and was not applied. Resend \"VERIFY\" "
                        "followed by exactly one COMPLETE fenced diff, small enough to fit in "
                        "one reply."
                    ),
                })
                current_phase = "explore"
                continue
            trial_diff = extract_diff(reply)
            if trial_diff is None:
                messages.append({
                    "role": "user",
                    "content": (
                        "That VERIFY had no ```diff fenced block -- resend as the line "
                        "\"VERIFY\" followed by exactly one fenced diff of the change to trial-compile."
                    ),
                })
                current_phase = "explore"
                continue
            applied, apply_msg = git_apply_fn(trial_diff, repo_root)
            if not applied:
                git_checkout_clean_fn(repo_root)
                messages.append({
                    "role": "user",
                    "content": (
                        f"VERIFY diff did not apply: {apply_msg}\n"
                        "Fix it and re-VERIFY, or submit your final diff."
                    ),
                })
                current_phase = "explore"
                continue
            check_ok, check_output = cargo_check_fn(repo_root)
            git_checkout_clean_fn(repo_root)
            tail = check_output[-DEFAULT_MAX_CHECK_OUTPUT_CHARS:]
            verdict = "PASSED" if check_ok else "FAILED"
            messages.append({
                "role": "user",
                "content": (
                    f"VERIFY result: cargo check {verdict}. The trial change has been REVERTED -- "
                    "the worktree is clean again, so your final diff must contain the complete change.\n"
                    f"{tail}"
                ),
            })
            current_phase = "explore"
            continue

        patch_match = PATCH_HEADER_RE.match(reply.strip())
        if patch_match:
            chunk_index, chunk_total = int(patch_match.group(1)), int(patch_match.group(2))
            if (
                patch_turns_used >= DEFAULT_MAX_PATCH_CHUNKS
                or chunk_index < 1 or chunk_total < 1 or chunk_index > chunk_total
            ):
                return (
                    False, "no diff in model response (patch chunking exceeded safety limit)",
                    None, messages,
                )
            patch_turns_used += 1
            # A chunk whose OWN ```diff fence never closed (this chunk's
            # reply hit config["max_tokens"] before it finished) would let
            # DIFF_BLOCK_UNCLOSED_RE's fallback (see extract_diff) silently
            # accept a partial chunk body -- corrupting the reassembled
            # diff the same way an unterminated final diff would (see the
            # check on that path below). Reject it exactly like a chunk
            # with no fence at all, rather than accepting a truncated one.
            if _reply_has_unterminated_diff_fence(reply):
                messages.append({
                    "role": "user",
                    "content": (
                        f"That \"PATCH {chunk_index}/{chunk_total}\" chunk cut off mid-diff -- its "
                        "```diff fence never closed, so what's captured is incomplete and would "
                        "corrupt the reassembled patch. Resend just this chunk, complete -- cut it "
                        "at an earlier point if it's still too long for one reply."
                    ),
                })
                current_phase = "patch"
                continue
            chunk_diff = extract_diff(reply)
            if chunk_diff is None:
                messages.append({
                    "role": "user",
                    "content": (
                        f"That \"PATCH {chunk_index}/{chunk_total}\" message didn't include a "
                        "```diff fenced block -- resend just this chunk with the diff content "
                        "included."
                    ),
                })
                current_phase = "patch"
                continue
            patch_chunks[chunk_index] = chunk_diff
            # Check completeness by CONTENT (every index 1..chunk_total
            # present), not by whether this reply's own index equals
            # chunk_total -- chunks can arrive out of order (e.g. N/N sent
            # before some earlier index), and checking only "is this the
            # last-numbered chunk" would otherwise miss a real gap left by
            # an out-of-order delivery, or re-request an already-received
            # chunk forever.
            missing = [i for i in range(1, chunk_total + 1) if i not in patch_chunks]
            if missing:
                missing_str = ", ".join(str(i) for i in missing)
                messages.append({
                    "role": "user",
                    "content": (
                        f"Received chunk {chunk_index}/{chunk_total}. Still missing chunk(s) "
                        f"{missing_str} -- send the next one now, in the same "
                        f"\"PATCH i/{chunk_total}\" + ```diff fenced block format."
                    ),
                })
                current_phase = "patch"
                continue
            diff = "".join(patch_chunks[i] for i in range(1, chunk_total + 1))
        else:
            # Checked BEFORE extract_diff, unconditionally -- never after.
            # extract_diff's DIFF_BLOCK_UNCLOSED_RE fallback (see #633) can
            # recover something from a reply whose ```diff fence never
            # closed, but content cut off by config["max_tokens"] is
            # incomplete BY CONSTRUCTION, and that something can look like
            # a perfectly valid, complete diff if the cut happened to land
            # on a hunk boundary: git apply then accepts it and silently
            # drops everything after the cut (measured directly -- a
            # 2-hunk diff truncated at the start of hunk 2 applies cleanly
            # as a 1-hunk diff). A plausible-but-wrong partial application
            # is worse than an outright apply failure, so a diff salvaged
            # from a truncated reply must NEVER reach git_apply_fn, and
            # exhausting the retry budget below fails the attempt outright
            # rather than falling through to try applying the salvage.
            if _reply_has_unterminated_diff_fence(reply):
                if truncation_retries_used < DEFAULT_MAX_TRUNCATION_RETRIES:
                    truncation_retries_used += 1
                    messages.append({
                        "role": "user",
                        "content": TRUNCATED_DIFF_RETRY_DEMAND,
                    })
                    current_phase = "patch"
                    continue
                return (
                    False,
                    "no diff in model response (reply truncated mid-diff even after "
                    "being told to use PATCH i/N chunking)",
                    None, messages,
                )
            diff = extract_diff(reply)
            if diff is None:
                return False, "no diff in model response", None, messages

        diff_attempts_used += 1
        applied, apply_msg = git_apply_fn(diff, repo_root)
        if not applied:
            git_checkout_clean_fn(repo_root)
            last_diff_failure = ("apply", apply_msg)
            patch_chunks = {}  # a resend may be a fresh single diff or a fresh chunk sequence
            messages.append({
                "role": "user",
                "content": f"That diff did not apply: {apply_msg}\nPlease resend a corrected diff.",
            })
            current_phase = "patch"
            continue

        built, build_err = cargo_build_fn(repo_root)
        if built:
            return True, None, diff, messages

        git_checkout_clean_fn(repo_root)
        last_diff_failure = ("build", build_err)
        patch_chunks = {}
        messages.append({
            "role": "user",
            "content": f"The build failed:\n{build_err}\nPlease resend a corrected diff.",
        })
        current_phase = "patch"

    reason = "no working fix after repair attempt"
    if last_diff_failure and last_diff_failure[0] == "apply":
        reason = annotate_rejection(
            reason, REJECT_PATCH_NOT_APPLIED,
            f"every diff was rejected by git apply (last error: {last_diff_failure[1]}). "
            "Nothing was ever written to disk, so this says nothing about whether the "
            "FIX is right -- resend as a minimal unified diff with correct a/ b/ paths "
            "and exact context lines, or request the file again to re-read its current "
            "contents.",
        )
    return False, reason, None, messages


def provider_slug(base_url):
    """Short, stable provider token for manifest lines ("theclawbay",
    "wafer", ...).

    The same model id can be served by more than one provider and behave
    completely differently: DeepSeek-V4-Pro scored 98.7% on wafer and
    81.8% on clawbay purely because clawbay hit a weekly spend cap and
    started 429ing. Without the provider on the line those two are one
    indistinguishable bucket, so a provider-level failure looks like the
    model degrading and the scoreboard blames the wrong thing.
    """
    if not base_url:
        return "unknown"
    host = re.sub(r"^https?://", "", str(base_url)).split("/")[0].split(":")[0]
    parts = [p for p in host.split(".") if p not in ("www", "api", "pass", "com", "ai", "io", "net")]
    return (parts[-1] if parts else host) or "unknown"


DEFAULT_MAX_REPAIR_ROUNDS = 5

#: EXTRA rounds, on top of max_repair_rounds, that only a substantive
#: reviewer rejection can unlock -- each rejection buys one more, up to
#: this many. (A reviewer that could not be reached, or whose reply
#: carried no verdict, has not rejected anything and buys nothing -- see
#: REVIEW_INFRA_PREFIXES.)
#: A fix the reviewer keeps pushing back on therefore gets up to
#: 5 + 5 = 10 rounds of genuine back-and-forth, while a fix that cannot
#: compile still gets exactly max_repair_rounds.
#:
#: Not simply "raise max_repair_rounds to 10", and the distinction is the
#: whole point. The two failure populations are not alike:
#:
#:   - A build failure, a gap-count miss or a test regression is the model
#:     failing against a machine that already told it exactly what was
#:     wrong. Five tries at that is generous; a sixth is usually the same
#:     wrong idea again, and doubling the allowance doubles the cost of
#:     every doomed target in the fleet.
#:   - A reviewer rejection is different in kind: the patch COMPILED, the
#:     gap count moved and the targeted tests passed (the full workspace
#:     suite runs only after approval -- see fix_gap's approved branch).
#:     Every mechanical gate that has run agreed. What is left is a judgment call about genuineness
#:     (hardcoded sample values, double emission, invented fixtures --
#:     see REVIEW_CHECKLIST), and that is exactly the argument worth
#:     having more than once, because the fixer is close and the
#:     disagreement is specific enough to act on.
#:
#: So the budget is spent where the conversation is productive rather than
#: spread evenly over failures that are not.
DEFAULT_MAX_REVIEW_ROUNDS = 5

#: fix_gap gives up on a target after this many CONSECUTIVE rounds whose
#: patch left the format's gap set byte-identical (see
#: REJECT_GAP_SET_UNCHANGED). Two, not one: the first such round can still
#: be a near miss the critique can steer, but a second in a row means the
#: worker cannot reach the thing it is aiming at, and every further round
#: is a fixer call plus a critique call spent proving that again.
MAX_GAP_SET_UNCHANGED_ROUNDS = 2
# 3000 was observed live keeping only leading compiler warnings for a
# workspace with many test binaries, cutting off before the actual
# "FAILURES:" section / panic detail cargo prints near the end -- the
# critique model (and a human debugging alongside it) never saw the real
# failure reason, just noise. Raised for more headroom, then superseded by
# _extract_test_failure_context below (a blind tail-keep of any fixed size
# still loses the real failure when OTHER unrelated test binaries print
# thousands of lines AFTER it in a workspace run).
DEFAULT_MAX_TEST_OUTPUT_CHARS = 8000

_TEST_FAILURE_MARKER_RE = re.compile(
    r"panicked at|assertion[^\n]*failed|^failures:|test result: FAILED|"
    r"^error(\[E\d+\])?:|\bFAILED\b"
)


def _extract_test_failure_context(output, max_chars=DEFAULT_MAX_TEST_OUTPUT_CHARS):
    """Pull the lines around real failure markers (panics, failed
    assertions, FAILED test names, the trailing "failures:" summary) out of
    a full cargo test run, instead of blindly keeping the last N chars.

    A `cargo test --workspace` run with many test binaries keeps executing
    binaries after the first failure, so the actual panic/assertion detail
    for the regression can be thousands of lines before the end of the
    output -- a tail-keep of ANY fixed size loses it and leaves only cargo's
    generic "error: test failed, to rerun pass `-p ... --test ...`" line,
    which names no failing assertion. This scans for marker lines instead
    and keeps a window of context around each one, plus the trailing chunk
    (where the real final summary usually is). Falls back to a blind tail
    if no markers are found, so any output shape this doesn't recognize
    degrades to the old behavior rather than returning nothing useful.
    """
    lines = output.splitlines()
    if not lines:
        return output[-max_chars:]

    marker_idx = [i for i, line in enumerate(lines) if _TEST_FAILURE_MARKER_RE.search(line)]
    if not marker_idx:
        return output[-max_chars:]

    windows = [(max(0, i - 2), min(len(lines), i + 13)) for i in marker_idx]
    windows.append((max(0, len(lines) - 30), len(lines)))
    windows.sort()

    merged = []
    for start, end in windows:
        if merged and start <= merged[-1][1] + 2:
            merged[-1] = (merged[-1][0], max(merged[-1][1], end))
        else:
            merged.append((start, end))

    extracted = "\n[...]\n".join("\n".join(lines[start:end]) for start, end in merged)
    if len(extracted) <= max_chars:
        return extracted
    half = max_chars // 2
    return extracted[:half] + "\n[...]\n" + extracted[-half:]

# Spec S3 [table_job] defaults: T3 TABLE-PORT / T4 FOUNDATION-UNLOCK jobs
# are scoped to a whole %table/module rather than one tag, so they get a
# bigger prompt budget and more repair rounds than a per-tag fix (see
# normalize_table_job_config).
DEFAULT_TABLE_JOB_MAX_PROMPT_TOKENS = 16384
DEFAULT_TABLE_JOB_MAX_REPAIR_ROUNDS = 8

# attempt_build's exception path is the single producer of this prefix
# (its `return False, f"model call failed: {e}", ...`); fix_gap and
# run_tag_loop both key off it to recognize infrastructure
# (rate-limit/network/provider) failures that say nothing about the tag
# or the diff.
INFRA_FAILURE_PREFIX = "model call failed:"

#: The reviewer-side counterparts: review_verdict's exception path
#: produces "review call failed: ..." and extract_review_verdict_full's
#: no-usable-verdict fallback produces "unparseable review verdict: ...".
#: Both reach fix_gap as a falsy verdict, but neither is a judgment about
#: the diff, so fix_gap must not let them extend the round budget or
#: enter the "still binding" rejection history (see its review_infra
#: branch).
REVIEW_INFRA_PREFIXES = ("review call failed:", "unparseable review verdict:")


# --- K5: reviewer evidence defaults -----------------------------------------

def _resolve_oxidex_binary(repo_root):
    """This worktree's own oxidex binary, or None. target/debug first,
    target/fixloop next (see cargo_build's "fixloop" profile -- the one
    this loop's own build step actually produces)."""
    repo_root = Path(repo_root)
    return next(
        (c for c in (repo_root / "target" / "debug" / "oxidex",
                     repo_root / "target" / "fixloop" / "oxidex") if c.is_file()),
        None,
    )


def default_extract_live_evidence(repo_root, sample_path, tag_keys):
    """K5 real default for fix_gap's extract_evidence_fn: shell out to the
    pinned ExifTool oracle and this worktree's own oxidex binary for just
    the target tags on one real sample file -- NOT the comparison JSON (whose
    matched_tags carries no values, the unimplementable-recheck-evidence
    critique this resolves). Renders "<tag>: exiftool=<v> oxidex=<v>
    (post-fix)" per tag found in either output.

    target/debug is tried first, target/fixloop next (see cargo_build's
    "fixloop" profile -- the one this loop's own build step actually
    produces). Best-effort throughout: a missing binary/exiftool/sample,
    or any parse failure, yields "" rather than raising -- reviewer
    evidence is advisory, never a hard dependency of the review call.
    """
    if not sample_path or not tag_keys:
        return ""
    repo_root = Path(repo_root)
    binary = _resolve_oxidex_binary(repo_root)
    if binary is None:
        return ""
    try:
        # The pinned oracle, not `shutil.which("exiftool")`: a PATH exiftool is
        # a different release from the one the tables were transcribed from, so
        # the "exiftool=" half of this evidence could disagree with oxidex for
        # reasons that have nothing to do with the fix under review. An
        # unresolvable/skewed/degraded oracle raises OracleError (a RuntimeError)
        # and lands in the same best-effort "" as a missing binary did before.
        oracle = shared_exiftool_oracle()
        et_proc = subprocess.run(  # nosec B603
            oracle.command(["-j", "-G", str(sample_path)]),
            capture_output=True, text=True, timeout=30,
        )
        ox_proc = subprocess.run(  # nosec B603
            [str(binary), "-j", "--exiftool-compat", str(sample_path)],
            capture_output=True, text=True, timeout=30,
        )
        # parse_float=str on BOTH sides. ExifTool's "1.80" becomes 1.8 under
        # default float handling, and this renders the two values side by side
        # with !r -- converting one side and not the other would print
        # `exiftool='1.80' oxidex=1.8` for a byte-identical pair and invite the
        # reviewer to reject a correct fix.
        et_tags = json.loads(et_proc.stdout, parse_float=str)[0] if et_proc.stdout.strip() else {}
        ox_tags = json.loads(ox_proc.stdout, parse_float=str)[0] if ox_proc.stdout.strip() else {}
    except (OSError, subprocess.SubprocessError, ValueError, IndexError, RuntimeError):
        return ""

    def lookup(tags, tag_key):
        name = tag_key.rsplit(":", 1)[-1]
        for k, v in tags.items():
            if k == tag_key or k.rsplit(":", 1)[-1] == name:
                return v
        return None

    lines = []
    for tag_key in tag_keys:
        et_val, ox_val = lookup(et_tags, tag_key), lookup(ox_tags, tag_key)
        if et_val is None and ox_val is None:
            continue
        lines.append(f"{tag_key}: exiftool={et_val!r} oxidex={ox_val!r} (post-fix)")
    return "\n".join(lines)


# --- Pre-dispatch reachability ----------------------------------------------
#
# oxidex emits File:* (and a handful of ungrouped) tags for ANY byte
# sequence at all -- run it on a plain text file and it still reports
# FileName, FileSize, FilePermissions. So "produces zero tags" is never
# literally true and cannot be the test. What IS true of a format whose
# parser never runs: every tag it emits comes from that generic
# filesystem path, and nothing is group-qualified by a real parser.
#
# That is what ISO 9660 looked like on 2026-07-30 at 0% parity: the parser
# was present and correct, but its signature sits at byte 32769 while
# detection only buffers 1 KiB, so the file resolved to Unknown and iso.rs
# never executed. Every patch to iso.rs was doomed before it was written,
# and the loop had no way to know.
#
# `File:FileType == "Unknown"` looks like the obvious test for that and is
# NOT usable as one -- measured against the live corpus on 2026-07-30, PSD,
# TTF, PE and XMP all report File:FileType=Unknown while their parsers run
# perfectly and emit 6, 6, 56 and 54 format-specific tags respectively.
# Skipping on that signal alone would have starved four healthy formats
# (386 open tags between them at the time) to save two. The emitted-tag
# test below is the one that separates them, so it is the only one that
# gates a skip; FileType is reported alongside as corroboration, never as
# a trigger.
#: Tag groups oxidex fills in from the filesystem regardless of whether
#: any format parser ran. A file whose entire output is drawn from these
#: has had no format-specific parsing done to it.
FILESYSTEM_TAG_FAMILIES = frozenset({"File", "SourceFile", "ExifTool", "System"})

#: File:FileType value oxidex reports when detection did not resolve the
#: file to any known format. Reported, never acted on -- see above.
UNKNOWN_FILE_TYPE = "Unknown"


def format_specific_tag_keys(tags, filesystem_families=FILESYSTEM_TAG_FAMILIES):
    """The keys of `tags` (one oxidex -j object) that came from a format
    parser rather than the generic filesystem path: sorted, group-qualified
    ("EXIF:Make"), excluding filesystem_families.

    Ungrouped keys (oxidex emits bare "FileSize"/"LineCount"/"WordCount"
    alongside the grouped copies) are excluded too -- they are duplicates
    of grouped entries or generic file facts, and counting them would make
    every file look parsed."""
    return sorted(
        k for k in (tags or {})
        if ":" in k and k.split(":", 1)[0] not in filesystem_families
    )


def default_format_reachable(fmt, sample_path, repo_root=None, run_fn=None, timeout=30):
    """Does oxidex parse `sample_path` as anything at all?

    Returns (reachable, detail):
      True  -- oxidex emits at least one format-specific tag for this
               sample, so a patch to its parser can move the gap.
      False -- oxidex refuses the file outright (non-zero exit, no output),
               or emits nothing outside FILESYSTEM_TAG_FAMILIES. Its parser
               is not running; no single-tag diff can close anything here.
      None  -- undetermined (no binary built yet, no sample on record, the
               subprocess failed or printed unparseable JSON). Callers MUST
               treat None as reachable: a skip on "we could not check" would
               silently stop the fleet the first time a worktree was mid-
               rebuild, which is strictly worse than the waste this avoids.

    Deliberately conservative in the same direction throughout: every
    ambiguous outcome is None, and File:FileType=Unknown -- which looks
    like the definitive signal and is not (see FILESYSTEM_TAG_FAMILIES'
    comment) -- never triggers a skip on its own. Verified on the live
    corpus 2026-07-30: False for ISO and Mach-O (the two formats hand-fixed
    that day for exactly this reason), True for all sixteen other formats
    the fleet had open tags on.

    run_fn(argv, timeout) -> (returncode, stdout) is injectable so tests
    never shell out; it defaults to subprocess.run.
    """
    if not fmt or not sample_path:
        return None, "no sample file on record for this format"
    binary = _resolve_oxidex_binary(repo_root or REPO_ROOT)
    if binary is None:
        return None, "no oxidex binary built in this worktree yet"

    def _run(argv, _timeout):
        proc = subprocess.run(  # nosec B603
            argv, capture_output=True, text=True, timeout=_timeout,
        )
        return proc.returncode, proc.stdout

    run_fn = run_fn or _run
    try:
        code, stdout = run_fn([str(binary), "-j", str(sample_path)], timeout)
    except (OSError, subprocess.SubprocessError, ValueError):
        return None, "oxidex could not be run on this sample"
    if code != 0 and not (stdout or "").strip():
        # oxidex refusing the file outright -- "Unsupported format: Format
        # Unknown not yet supported" (ISO) or a parser that errored before
        # emitting anything (Mach-O). Nothing downstream of detection ran.
        return False, (
            f"oxidex exited {code} with no output for {sample_path} -- nothing "
            f"downstream of format detection runs for {fmt}, so no patch to its "
            "parser can move this gap"
        )
    try:
        tags = json.loads(stdout)
        tags = tags[0] if isinstance(tags, list) else tags
    except (ValueError, IndexError):
        return None, "oxidex output was not parseable JSON"
    if not isinstance(tags, dict):
        return None, "oxidex output was not parseable JSON"

    specific = format_specific_tag_keys(tags)
    file_type = tags.get("File:FileType")
    if not specific:
        return False, (
            f"oxidex emits no format-specific tags for {sample_path} -- only "
            f"{'/'.join(sorted(FILESYSTEM_TAG_FAMILIES))} filesystem tags "
            f"(File:FileType={file_type!r}). The {fmt} parser produced nothing, so "
            "no patch to it can move this gap"
        )
    return True, (
        f"oxidex emits {len(specific)} format-specific tag(s) for {sample_path} "
        f"(File:FileType={file_type!r})"
    )


def format_unreachable_reason(fmt, detail=""):
    """The single rejection string for a target skipped pre-dispatch --
    annotated with REJECT_FORMAT_UNREACHABLE so the distiller and every
    dashboard can count these separately from real patch failures."""
    return annotate_rejection(
        f"format {fmt} produces no output; parser unreachable",
        REJECT_FORMAT_UNREACHABLE, detail,
    )


def make_reachability_fn(repo_root=None, check_fn=None, log_fn=None):
    """Build run_tag_loop's reachability_fn: a per-format-memoized
    (tag_gap) -> (reachable, detail) closure over default_format_reachable.

    Memoized because the answer is a property of the worktree's binary and
    the format's own sample, not of the tag -- re-running oxidex for every
    one of a format's several hundred open tags would cost more than the
    calls it saves. The memo lives for one process; a worker is respawned
    often enough that a format which becomes reachable mid-run is picked up
    on the next spawn, and an UNREACHABLE verdict blacklists the tag anyway
    (see run_tag_loop), so a stale positive is the only direction this can
    err and it errs toward doing the work.
    """
    check_fn = check_fn or default_format_reachable
    cache = {}

    def reachability_fn(tag_gap):
        fmt = tag_gap.get("format")
        if fmt not in cache:
            sample = (tag_gap.get("entry") or {}).get("source_file")
            cache[fmt] = check_fn(fmt, sample, repo_root)
            if log_fn:
                reachable, detail = cache[fmt]
                log_fn(f"[{fmt}] reachability: {reachable} -- {detail}")
        return cache[fmt]

    return reachability_fn


def default_emission_scan(repo_root, parser_files, tag_keys, diff_text=None):
    """K5 real default for fix_gap's scan_fn: `rg -n` over this gap's
    OWN parser-file directories (never a repo-wide grep, which would
    fill the reviewer's context with other manufacturers' unrelated
    hits), one search per target tag name, plus -- when diff_text is
    given -- the same pre/post occurrence-count machinery
    detect_duplicate_tag_insertion uses for every file the diff touches.
    Best-effort: a missing `rg`/unreadable file yields no line for that
    tag/file rather than raising."""
    if not tag_keys:
        return ""
    repo_root = Path(repo_root)
    dirs = sorted({str((repo_root / f).parent) for f in (parser_files or [])})
    lines = []
    if dirs:
        for tag_key in tag_keys:
            name = tag_key.rsplit(":", 1)[-1]
            try:
                result = subprocess.run(  # nosec B603
                    ["rg", "-n", "-F", name, *dirs],
                    capture_output=True, text=True, timeout=15,
                )
            except (OSError, subprocess.SubprocessError):
                continue
            hits = [ln for ln in result.stdout.splitlines() if ln.strip()]
            lines.append(f"{tag_key}: {len(hits)} occurrence(s) in the parser subtree"
                         + ("\n" + "\n".join(hits[:5]) if hits else ""))
    if diff_text:
        for path in DIFF_FILE_HEADER_RE.findall(diff_text):
            full_path = repo_root / path
            try:
                post_text = full_path.read_text()
            except OSError:
                continue
            pre_text = file_content_at_head(path, repo_root)
            for tag_key in tag_keys:
                name = tag_key.rsplit(":", 1)[-1]
                lines.append(
                    f"{path}: {tag_key} occurrences pre={pre_text.count(name)} "
                    f"post={post_text.count(name)}"
                )
    return "\n".join(lines)


# --- K1 lesson writers + M3 recheck classification (fix_gap helpers) -------

def _gap_primary_tag_key(gap):
    """Best tag_key for a K1 lesson from a fix_gap gap dict (single or
    clustered) -- the first entry across missing_tags then
    value_differences; "" when the gap somehow has neither."""
    for e in gap["missing_tags"]:
        return f"{e['family']}:{e['name']}"
    for e in gap["value_differences"]:
        return e["tag_key"]
    return ""


def _gap_member_tag_gaps(gap):
    """Reconstruct tag_still_open-shaped {"kind", "entry"} member dicts
    from a fix_gap gap's own missing_tags/value_differences lists (a
    fix_gap gap may bundle several tags together -- see
    make_cluster_gap)."""
    members = [{"kind": "missing", "entry": e} for e in gap["missing_tags"]]
    members += [{"kind": "diff", "entry": e} for e in gap["value_differences"]]
    return members


def _member_tag_key(member):
    entry = member["entry"]
    if member["kind"] == "missing":
        return f"{entry['family']}:{entry['name']}"
    return entry["tag_key"]


def _classify_recheck_failure(gap, post_match):
    """Best-effort classification of why a recheck still shows this gap
    open, for the K1 lesson event (spec items 2/9): "structural" when
    tag_still_open flags a duplicate emission (M3 -- checked first, it's
    the deterministic signal), "wrong_value" with the live exiftool/
    oxidex values when a member is present-but-wrong, else the generic
    "gap_not_closed". post_match=None (the legacy 1-/2-tuple recheck_fn
    contract most existing callers/tests still use) always falls back to
    "gap_not_closed" -- there is nothing structured here to classify.
    Returns (event, evidence_or_None, tag_key_or_None)."""
    if post_match is None:
        return "gap_not_closed", None, None
    members = _gap_member_tag_gaps(gap)
    for member in members:
        if tag_still_open(post_match, member) == ("duplicate_emission",):
            return "structural", None, _member_tag_key(member)
    for member in members:
        verdict = tag_still_open(post_match, member)
        if verdict and verdict[0] == "value_differs":
            return (
                "wrong_value",
                {"exiftool_value": verdict[1], "oxidex_value": verdict[2]},
                _member_tag_key(member),
            )
    return "gap_not_closed", None, None


def _write_fix_gap_lesson(knowledge_home, worker_label, event, reason, format_name, *,
                          evidence=None, tag_key=None, checklist_id=None,
                          module=None, table=None, now_fn=time.time):
    """Best-effort K1 lesson append (spec item 2): a lesson-append
    failure must NEVER break the fixer loop. knowledge_home=None (the
    default everywhere in this module) is a no-op -- lesson writing is
    strictly opt-in, same hermetic-by-default contract as build_prompt's
    knowledge_home."""
    if knowledge_home is None:
        return
    try:
        event_dict = make_lesson_event(
            ts=now_fn(), worker=worker_label, format_name=format_name,
            module=module, table=table, event=event, reason=reason,
            evidence=evidence or "", tag_key=tag_key or "", checklist_id=checklist_id or "",
        )
        append_lesson(knowledge_home, event_dict)
    except OSError:
        pass


def _parse_live_evidence_value(live_evidence, tag_key):
    """Best-effort extraction of one tag's (exiftool_value, oxidex_value)
    back out of default_extract_live_evidence's rendered text, for the
    M1 Exiftool-Value/Oxidex-Value trailers. (None, None) when not found
    or live_evidence is empty -- evidence is advisory, so a trailer this
    can't recover simply isn't emitted (spec M1's "else omit")."""
    if not live_evidence or not tag_key:
        return None, None
    m = re.search(
        re.escape(tag_key) + r": exiftool=(.*?) oxidex=(.*?) \(post-fix\)", live_evidence,
    )
    if not m:
        return None, None
    return m.group(1), m.group(2)


def _perl_ref_from_block(perl_block_text):
    """Best-effort pm-file for the M1 Perl-Ref trailer, parsed from
    build_perl_reference_block's own snippet header (see
    PERL_MODULE_HEADER_RE) -- None (omit) when perl_block_text is empty
    (no --perl-lib resolved) or carries no recognizable module header.
    The snippet header itself carries no line number, so this is always
    just "<pm-file>", never "<pm-file>:<line>" -- a real limitation of
    extract_perl_tag_snippet's current return shape, not something this
    function guesses at."""
    if not perl_block_text:
        return None
    m = PERL_MODULE_HEADER_RE.search(perl_block_text)
    return m.group(1) if m else None


def _build_fix_gap_trailers(gap, fmt, tag_keys, sample_path, live_evidence, perl_block_text,
                            gap_count_before, remaining_after, worker_label, table_name,
                            review_flags):
    """Assemble the M1 evidence-trailer list for a landed fix_gap commit:
    an ordered [(key, value), ...] list (git_commit's own contract,
    which is what lets a cluster commit carry multiple Tag: entries).
    Omittable trailers (Sample/Exiftool-Value/Oxidex-Value/Perl-Ref/
    Worker/Table/Review-Unverifiable) are simply absent from the list
    when their evidence isn't available -- git_commit already skips any
    falsy value defensively too."""
    trailers = [("Format", fmt)]
    for tag_key in tag_keys:
        trailers.append(("Tag", tag_key))
    if sample_path:
        trailers.append(("Sample", sample_path))
    if tag_keys:
        et_val, ox_val = _parse_live_evidence_value(live_evidence, tag_keys[0])
        if et_val is not None:
            trailers.append(("Exiftool-Value", et_val))
        if ox_val is not None:
            trailers.append(("Oxidex-Value", ox_val))
    perl_ref = _perl_ref_from_block(perl_block_text)
    if perl_ref:
        trailers.append(("Perl-Ref", perl_ref))
    trailers.append(("Verified", f"recheck-pass gaps={gap_count_before}->{remaining_after}"))
    if worker_label:
        trailers.append(("Worker", worker_label))
    if table_name:
        trailers.append(("Table", table_name))
    if review_flags:
        trailers.append(("Review-Unverifiable", ",".join(review_flags)))
    return trailers


def fix_gap(gap, config, *, call_model_fn=call_model, review_call_model_fn=None,
            critique_call_model_fn=None,
            git_apply_fn=git_apply,
            git_checkout_clean_fn=git_checkout_clean, git_commit_fn=git_commit,
            cargo_build_fn=cargo_build, cargo_test_workspace_fn=cargo_test_workspace,
            cargo_test_targeted_fn=cargo_test_targeted,
            cargo_check_fn=cargo_check,
            attempt_build_fn=attempt_build, review_fn=review_verdict,
            critique_fn=critique_failed_attempt,
            pick_model_fn=random.choice, log_fn=print,
            review_config=None, recheck_fn=None, repo_root=None, samples_dir=None,
            previous_attempts=None, detect_duplicate_fn=detect_duplicate_tag_insertion,
            perl_lib_dir=None, sweep_review_log_path=None,
            neighbor_precedent_block="",
            max_repair_rounds=DEFAULT_MAX_REPAIR_ROUNDS,
            max_review_rounds=DEFAULT_MAX_REVIEW_ROUNDS,
            knowledge_home=None, module_name=None, table_name=None, worker_label=None,
            recheck_baseline=None, extract_evidence_fn=None, scan_fn=None):
    """Attempt to close one format's gaps over a series of candidates,
    each round feeding the previous round's outcome -- build error, gap
    count, test regression, or review rejection -- plus a
    critique_fn-generated critique back into the conversation before
    trying again.

    The round budget starts at max_repair_rounds and is EXTENDED by one
    per reviewer rejection, up to max_review_rounds extra: a candidate the
    reviewer keeps arguing with gets up to max_repair_rounds +
    max_review_rounds (5 + 5 = 10 by default) rounds of back-and-forth,
    while one that merely fails to compile still gets max_repair_rounds.
    See DEFAULT_MAX_REVIEW_ROUNDS for why the two are budgeted separately
    rather than by just doubling max_repair_rounds. Each retry carries the
    full accumulated list of rejections, not only the newest, and reminds
    the fixer that reading files is free (see attempt_build) so it can
    re-read before resending instead of guessing.

    Returns a result dict whose "rounds" key is the full
    per-round history (diff attempted, failure reason, critique), not
    just the last one, so run_tag_loop can persist all of it for future
    rounds targeting this same tag (see format_previous_attempts).

    Every round that doesn't end in "fixed" gets a critique -- from
    critique_fn for a build/gap/test failure (which never reaches
    review_fn, since there's no successful build yet to judge for
    genuineness), or from review_fn's own rejection reason once a
    candidate does build and test cleanly. Previously, only a
    review-rejected candidate got this kind of feedback; a build failure
    or gap-count/test-regression just failed immediately with a short
    mechanical reason and no chance to course-correct within the same
    call. Course-correcting only across separate run_tag_loop rounds
    (which start a fresh conversation each time) meant the model never
    saw *why* its specific approach was wrong until this session's
    persisted-history work (see load_recent_sweep_reviews) started
    carrying reasons forward -- and even then, only for a later round on
    the same tag, not within the attempt that just failed.

    review_config, if provided, is the config dict used for the review
    call instead of the fixer's own config -- lets the outer loop's
    reviewer run on a different model/provider than the fixer. Defaults
    to reusing config, matching the original single-config behavior.

    review_call_model_fn, if provided, is used for review_fn's call
    instead of call_model_fn -- lets a caller distinguish fixer vs
    reviewer calls in its own logging/metrics (see main()'s three
    phase-tagged logging_call_model closures) despite both ultimately
    calling the same underlying call_model. Defaults to call_model_fn,
    matching the original shared-closure behavior.

    critique_call_model_fn, if provided, is used for critique_fn's call
    the same way -- defaults to call_model_fn.

    pick_model_fn is threaded into both attempt_build_fn and review_fn, so
    a single injected fake can make an entire fix_gap call deterministic
    in tests despite config["models"] holding multiple entries.

    cargo_check_fn is threaded to attempt_build_fn for the VERIFY protocol.

    cargo_test_targeted_fn(repo_root, filter_str) -> (success, output) is
    the cheap first-line test gate (cargo test --lib <format lowercased>)
    run right after the gap-count gate; the full workspace suite via
    cargo_test_workspace_fn only runs once a candidate has survived
    review, immediately before the commit.

    log_fn(str) is called with a one-line status update at every decision
    point (build result, gap delta, review verdict, commit) -- defaults to
    print, so `--only-format`'s stdout (which parallel_model_fix_loop.py
    redirects to a per-format log file) carries a live, parseable trail of
    what this attempt is doing. Pass a no-op to silence it (e.g. in tests).

    recheck_fn(format_name) -> int must return the gap count for that
    format after the attempted fix (used to confirm real progress). If not
    provided, progress can never be confirmed and the attempt always fails
    the "gap count did not decrease" check. It may instead return a
    (count, detail) tuple, where a non-None detail string replaces the
    generic "gap count did not decrease" as the failure reason.

    On the 3-tuple contract (see recheck_baseline below), a failed recheck
    is additionally classified by classify_recheck_rejection and the
    resulting rejection code stamped onto the reason: wrong-value,
    gap-set-churned (the count is flat but the SET moved -- the patch DID
    work), or gap-set-unchanged (nothing moved at all). Two CONSECUTIVE
    gap-set-unchanged rounds end the attempt immediately with
    result["rejection_code"] set, rather than spending the remaining
    repair rounds -- see MAX_GAP_SET_UNCHANGED_ROUNDS.

    previous_attempts, if given, is passed straight through to build_prompt
    (see format_previous_attempts) -- prior rounds' diffs/failure reasons
    for this exact gap, so a repair round-trip driven by run_tag_loop's
    persisted per-tag history doesn't repeat the same broken approach.

    detect_duplicate_fn(diff_text, tag_literal, repo_root) -> bool is
    checked right after a candidate diff builds and passes cargo test,
    but BEFORE spending a reviewer call on it: this is the review step's
    own defense against a worker whose worktree was stale when it
    started this attempt (see run_tag_loop's per-round
    refresh_worktree_fn, which shrinks but can't fully close that
    window) and has just independently reproduced a fix another worker
    already landed. A detected duplicate short-circuits straight to
    status "duplicate" -- distinct from "failed" so run_tag_loop knows
    not to count it against this tag's fail budget; it isn't this tag's
    fault that another worker got there first.

    perl_lib_dir, if given, is passed straight through to build_prompt
    (see resolve_exiftool_perl_lib_dir/build_perl_reference_block) so the
    initial prompt includes ExifTool's own Perl source for this gap's
    tags, not just on a repair round-trip.

    sweep_review_log_path, if given, is passed straight through to
    build_prompt (see load_recent_sweep_reviews/format_sweep_review_history)
    so the prompt includes recent human sweep-review verdicts for this format.

    knowledge_home/module_name, if given, are passed straight through to
    build_prompt (spec K2/K3 -- GLOBAL-PITFALLS.md + module playbook +
    lessons tail) AND double as the K1 lesson-writer destination/module
    tag below. table_name, when known (Phase 2 attribution isn't wired
    yet, so this is always None today), rides into both the K1 module+
    table fields and the M1 Table: trailer. worker_label identifies this
    process in every K1 lesson and the M1 Worker: trailer. None (the
    default) for any of these keeps every existing caller's behavior
    exactly as it was -- build_prompt omits the sections, and every K1
    lesson write below is a no-op (see _write_fix_gap_lesson).

    recheck_baseline, if given, is the pre-attempt per-format comparison
    dict for this gap's format (same shape as recheck_fn's optional 3rd
    tuple element below) -- spec M3's structural double-emission gate:
    when recheck_fn also supplies the POST-attempt comparison dict,
    new_oxidex_only_keys(recheck_baseline, post) is checked every round,
    and any newly-introduced oxidex-only tag fails the attempt (event
    "structural" in the K1 ledger) even if the gap count itself
    decreased. None (the default, matching every existing test's
    recheck_fn) skips this check entirely.

    extract_evidence_fn(repo_root, sample_path, tag_keys) -> str and
    scan_fn(repo_root, parser_files, tag_keys, diff_text) -> str (spec
    K5) are threaded into the reviewer call as live_evidence/
    emission_scan -- default to default_extract_live_evidence/
    default_emission_scan (real subprocess-backed implementations; see
    their own docstrings) when not given.

    neighbor_precedent_block, a pre-rendered string, is passed straight
    through to build_prompt (see build_neighbor_precedent_block) -- built
    by the caller so fix_gap stays free of git subprocess calls.
    """
    repo_root = repo_root or REPO_ROOT
    review_config = review_config or config
    review_call_model_fn = review_call_model_fn or call_model_fn
    critique_call_model_fn = critique_call_model_fn or call_model_fn
    extract_evidence_fn = extract_evidence_fn or default_extract_live_evidence
    scan_fn = scan_fn or default_emission_scan
    fmt = gap["format"]

    # Computed once and reused for both the fixer prompt (build_prompt
    # resolves its own copy internally from perl_lib_dir) and the K5
    # reviewer evidence below -- pure text extraction, not a model call,
    # so recomputing is cheap but there's no reason to.
    perl_block_text = build_perl_reference_block(gap, perl_lib_dir)
    target_tag_keys = [_member_tag_key(m) for m in _gap_member_tag_gaps(gap)]
    sample_path = next(
        (e.get("source_file") for e in (gap["missing_tags"] + gap["value_differences"])
         if e.get("source_file")),
        None,
    )

    messages = [{"role": "user", "content": build_prompt(
        gap, repo_root=repo_root,
        # A clustered gap (see make_cluster_gap) must show EVERY member
        # tag, even when max_prompt_tags is 1 -- the whole point of the
        # cluster is one conversation covering the sibling family.
        max_tags=(
            (len(gap["missing_tags"]) + len(gap["value_differences"]))
            if gap.get("clustered") else config["max_prompt_tags"]
        ),
        max_file_bytes=config["max_prompt_file_bytes"],
        samples_dir=samples_dir,
        perl_lib_dir=perl_lib_dir,
        sweep_review_log_path=sweep_review_log_path,
        previous_attempts=previous_attempts,
        max_prompt_tokens=config.get("max_prompt_tokens", DEFAULT_MAX_PROMPT_TOKENS),
        max_reply_tokens=config.get("max_tokens", DEFAULT_MAX_TOKENS),
        neighbor_precedent_block=neighbor_precedent_block,
        knowledge_home=knowledge_home, module_name=module_name,
        learning_budget_tokens=config.get("learning_budget_tokens", DEFAULT_LEARNING_BUDGET_TOKENS),
        parser_floor_tokens=config.get("parser_floor_tokens", DEFAULT_PARSER_FLOOR_TOKENS),
        lessons_tail_kb=config.get("lessons_tail_kb", DEFAULT_LESSONS_TAIL_KB),
        # Section 6: gates this worker's OWN quarantine verdicts and the
        # adaptive response-format remediation -- see build_prompt.
        worker_label=worker_label,
    )}]

    rounds = []  # every non-fixed round: {"diff", "reason", "critique"} -- see run_tag_loop
    diff = None
    # Consecutive rounds whose patch left the format's gap SET completely
    # untouched. Only ever non-zero on the 3-tuple recheck_fn contract
    # (recheck_baseline + post_match both present) -- see the gate below.
    gap_set_unchanged_rounds = 0

    # The round budget is not fixed: it STARTS at max_repair_rounds and
    # each reviewer rejection extends it by one, up to max_review_rounds
    # extra (see DEFAULT_MAX_REVIEW_ROUNDS). Only rejections extend it --
    # a build failure or test regression consumes a round without buying
    # another -- so `rounds_allowed` is read, never assumed, by every
    # last-round check below including critique_and_continue's.
    rounds_allowed = max_repair_rounds
    review_extensions_used = 0
    # Every reviewer rejection so far, oldest first. Fed back verbatim on
    # each retry: across ten rounds a fixer shown only the LATEST objection
    # will happily reintroduce the thing it was rejected for in round 2 to
    # satisfy the objection from round 7, and then oscillate between the
    # two until the budget is gone. Observed on the 5-round loop already;
    # doubling the rounds without doubling the memory would only make it a
    # longer oscillation.
    review_rejections = []

    def lesson(event, reason, **kwargs):
        """K1 writer shorthand bound to this call's fixed identity
        fields (best-effort -- see _write_fix_gap_lesson)."""
        _write_fix_gap_lesson(
            knowledge_home, worker_label, event, reason, fmt,
            module=module_name, table=table_name, **kwargs,
        )

    def critique_and_continue(failure_kind, reason, round_index):
        """Shared tail for every non-"fixed"/"duplicate" outcome: get a
        critique, record the round, and either return a final failure
        dict (last round) or append a repair turn and let the caller's
        loop continue to the next round.

        An infrastructure failure (reason starts with
        INFRA_FAILURE_PREFIX) skips critique_fn entirely and uses the
        reason itself as the critique: critiquing a rate-limit error
        wastes a model call that will usually itself be rate-limited,
        and produces no signal about the tag or the diff.

        Spec K1: ALSO writes its own lesson event ("infra" for an
        infrastructure failure, "critique" otherwise, carrying the
        critique text itself as the reason) -- distinct from the more
        specific event (build_failed/test_regressed/wrong_value/
        gap_not_closed/structural) the caller already wrote right before
        calling this."""
        if reason.startswith(INFRA_FAILURE_PREFIX):
            critique = reason
            lesson("infra", critique, tag_key=_gap_primary_tag_key(gap))
        else:
            critique = critique_fn(
                gap, diff, failure_kind, reason, config,
                call_model_fn=critique_call_model_fn, pick_model_fn=pick_model_fn,
            )
            lesson("critique", critique, tag_key=_gap_primary_tag_key(gap))
        rounds.append({"diff": diff, "reason": reason, "critique": critique})
        if round_index >= rounds_allowed - 1:
            return {"format": fmt, "status": "failed", "reason": reason, "diff": diff, "rounds": rounds}
        messages.append({
            "role": "user",
            "content": (
                f"That attempt failed ({failure_kind}): {reason}\n\n"
                f"Reviewer critique: {critique}\n\n"
                "Please resend a corrected diff."
            ),
        })
        return None

    # while, not `for ... in range(max_repair_rounds)`: rounds_allowed
    # grows as the reviewer rejects (see DEFAULT_MAX_REVIEW_ROUNDS), and a
    # range() snapshots its bound at loop entry.
    round_index = -1
    while round_index + 1 < rounds_allowed:
        round_index += 1
        built, reason, diff, messages = attempt_build_fn(
            messages,
            call_model_fn=call_model_fn, git_apply_fn=git_apply_fn,
            git_checkout_clean_fn=git_checkout_clean_fn, cargo_build_fn=cargo_build_fn,
            config=config, repo_root=repo_root, pick_model_fn=pick_model_fn,
            samples_dir=samples_dir,
            cargo_check_fn=cargo_check_fn,
        )
        if not built:
            log_fn(f"[{fmt}] build failed: {reason}")
            # An infrastructure failure is NOT a build failure -- from in
            # here a DNS error and a type error look identical, but only
            # one of them is a lesson. critique_and_continue below writes
            # the single `infra` row for it; writing a `build_failed` row
            # too is what put 218 duplicate outage reasons on the live
            # ledger (2026-07-25), each one a knowledge-file bullet and a
            # line of some worker's prompt budget spent on a 429.
            # distill_lessons.classify_event would now relabel the row
            # anyway; not writing it at all also saves the append.
            if not reason.startswith(INFRA_FAILURE_PREFIX):
                lesson("build_failed", reason, tag_key=_gap_primary_tag_key(gap))
            outcome = critique_and_continue("build_failed", reason, round_index)
            if outcome:
                return outcome
            continue

        recheck_result = recheck_fn(fmt) if recheck_fn else gap["gap_count"]
        recheck_detail = None
        post_match = None
        if isinstance(recheck_result, tuple):
            # Spec M3: a 3rd element is the raw post-attempt comparison
            # dict for this format (same shape recheck_baseline carries),
            # enabling both the structural double-emission classification
            # below and the new_oxidex_only_keys gate. Legacy 2-tuple
            # (count, detail) callers -- most existing tests -- are
            # unaffected; post_match just stays None for them.
            if len(recheck_result) >= 3:
                remaining, recheck_detail, post_match = (
                    recheck_result[0], recheck_result[1], recheck_result[2],
                )
            else:
                remaining, recheck_detail = recheck_result
        else:
            remaining = recheck_result
        log_fn(f"[{fmt}] gaps {gap['gap_count']} -> {remaining}")

        if recheck_baseline is not None and post_match is not None:
            introduced = new_oxidex_only_keys(recheck_baseline, post_match)
            if introduced:
                git_checkout_clean_fn(repo_root)
                reason = f"introduced new oxidex-only tag(s): {', '.join(introduced)}"
                log_fn(f"[{fmt}] {reason}, reverting")
                lesson("structural", reason, tag_key=_gap_primary_tag_key(gap))
                outcome = critique_and_continue("gap_not_closed", reason, round_index)
                if outcome:
                    return outcome
                continue

        if remaining >= gap["gap_count"]:
            git_checkout_clean_fn(repo_root)
            reason = recheck_detail or "gap count did not decrease"
            # Compare the SET of open gaps, not just the count. The count
            # is flat in two completely different situations -- "the patch
            # did nothing" and "the patch closed A and the rebuild revealed
            # B" -- and telling a worker in the second situation that its
            # gap count did not decrease is how a working approach gets
            # abandoned. classify_recheck_rejection stamps the reason with
            # which one this is; the count still governs the gate, because
            # the count is what the commit's Verified: trailer promises.
            # Both dicts absent (the legacy int/2-tuple recheck_fn
            # contract, which is what every pre-existing caller and test
            # uses) leaves the reason byte-for-byte as it has always been.
            tag_verdicts = (
                [tag_still_open(post_match, m) for m in _gap_member_tag_gaps(gap)]
                if post_match is not None else None
            )
            reason, reject_code, delta = classify_recheck_rejection(
                reason, recheck_baseline, post_match, tag_verdicts,
            )
            if delta:
                log_fn(f"[{fmt}] gap set delta: {format_gap_set_delta(delta)}")
            log_fn(f"[{fmt}] {reason}, reverting")
            gap_set_unchanged_rounds = (
                gap_set_unchanged_rounds + 1 if reject_code == REJECT_GAP_SET_UNCHANGED else 0
            )
            if gap_set_unchanged_rounds >= MAX_GAP_SET_UNCHANGED_ROUNDS:
                # Two consecutive rounds where nothing in the format's gap
                # set moved. The remaining repair rounds are a fixer call
                # plus a critique call each, spent re-deriving a change
                # whose predecessors were already proven to have no
                # observable effect. Stop and say so, so run_tag_loop
                # persists a reason the NEXT round for this tag can read.
                lesson("gap_not_closed", reason, tag_key=_gap_primary_tag_key(gap))
                log_fn(
                    f"[{fmt}] abandoning after {gap_set_unchanged_rounds} rounds with an "
                    "unchanged gap set -- target is structurally out of reach"
                )
                rounds.append({"diff": diff, "reason": reason, "critique": GAP_SET_UNCHANGED_ADVICE})
                return {
                    "format": fmt, "status": "failed", "reason": reason,
                    "diff": diff, "rounds": rounds, "rejection_code": reject_code,
                }
            # Spec M3/K1: classify via tag_still_open's own verdict when
            # the recheck_fn contract supplies a post-attempt comparison
            # dict -- "wrong_value" (with the live values as evidence)
            # over the generic "gap_not_closed" when a member is
            # present-but-wrong; "structural" when a member reads back as
            # a duplicate emission.
            event, evidence, lesson_tag_key = _classify_recheck_failure(gap, post_match)
            lesson(event, reason, evidence=evidence, tag_key=lesson_tag_key or _gap_primary_tag_key(gap))
            outcome = critique_and_continue("gap_not_closed", reason, round_index)
            if outcome:
                return outcome
            continue

        t_ok, t_out = cargo_test_targeted_fn(repo_root, fmt.lower())
        if not t_ok:
            git_checkout_clean_fn(repo_root)
            reason = f"targeted tests ({fmt.lower()}) regressed:\n{_extract_test_failure_context(t_out)}"
            log_fn(f"[{fmt}] targeted tests regressed, reverting")
            lesson("test_regressed", reason, tag_key=_gap_primary_tag_key(gap))
            outcome = critique_and_continue("test_regressed", reason, round_index)
            if outcome:
                return outcome
            continue

        tag_literal = tag_literal_for_gap(gap)
        if tag_literal and detect_duplicate_fn(diff, tag_literal, repo_root):
            git_checkout_clean_fn(repo_root)
            reason = f"duplicate: a handler for {tag_literal} already exists elsewhere"
            log_fn(f"[{fmt}] {reason}, reverting (not a failure -- another worker got there first)")
            lesson("duplicate", reason, tag_key=_gap_primary_tag_key(gap))
            return {"format": fmt, "status": "duplicate", "reason": reason, "diff": diff, "rounds": rounds}

        # Spec K5: live post-fix re-extraction and a scoped emission scan,
        # both best-effort (a failure here degrades to "" -- reviewer
        # evidence is advisory, never a hard dependency of the review call).
        try:
            live_evidence = extract_evidence_fn(repo_root, sample_path, target_tag_keys)
        except Exception:
            live_evidence = ""
        try:
            emission_scan = scan_fn(repo_root, gap["parser_files"], target_tag_keys, diff)
        except Exception:
            emission_scan = ""

        approved, review_reason = review_fn(
            gap, diff, review_config, call_model_fn=review_call_model_fn, pick_model_fn=pick_model_fn,
            perl_block=perl_block_text, live_evidence=live_evidence, emission_scan=emission_scan,
        )
        if approved:
            tests_passed, test_output = cargo_test_workspace_fn(repo_root)
            if not tests_passed:
                git_checkout_clean_fn(repo_root)
                # Failure detail (which assertion, panic message) can land
                # anywhere in a 2000+-test workspace run -- other binaries
                # keep executing after the real failure, so a blind tail
                # keep can lose it. Scan for FAILED/panic markers instead.
                tail = _extract_test_failure_context(test_output)
                reason = f"cargo test --workspace regressed:\n{tail}"
                log_fn(f"[{fmt}] cargo test --workspace regressed, reverting")
                lesson("test_regressed", reason, tag_key=_gap_primary_tag_key(gap))
                outcome = critique_and_continue("test_regressed", reason, round_index)
                if outcome:
                    return outcome
                continue
            closed = gap["gap_count"] - remaining

            # Spec K5: UNVERIFIABLE reviewer replies still land (review_fn
            # folds them into approved=True), but C1/C2 -- the two
            # checklist items whose failure means we could not actually
            # verify correctness -- route this commit to the human
            # judgment queue via review_flags + the M1 Review-Unverifiable
            # trailer (see review_fn/extract_review_verdict_full).
            #
            # A reply can say UNVERIFIABLE without a parseable C1-C5 token
            # (the prompt requires one, but a model formatting slip is a
            # real possibility -- extract_review_verdict_full already
            # anticipates this with its "no checklist id given" fallback
            # reason). checklist_id is then None, and None is never in
            # ("C1", "C2"), so treating that case as "not C1/C2" would
            # silently drop the safety net for the exact reply where the
            # model told us it couldn't verify the fix but we can't tell
            # which checklist item -- possibly C1/C2 -- was the reason.
            # Fail safe like every other unparseable-verdict path in this
            # file: unknown severity escalates to the human queue too,
            # tagged UNVERIFIABLE:UNKNOWN rather than silently passing.
            review_flags = []
            if review_reason.startswith("UNVERIFIABLE:"):
                checklist_id = parse_checklist_id(review_reason)
                if checklist_id in ("C1", "C2") or checklist_id is None:
                    review_flags.append(f"UNVERIFIABLE:{checklist_id or 'UNKNOWN'}")

            trailers = _build_fix_gap_trailers(
                gap, fmt, target_tag_keys, sample_path, live_evidence, perl_block_text,
                gap["gap_count"], remaining, worker_label, table_name, review_flags,
            )
            git_commit_fn(
                f"fix({fmt.lower()}): wire {closed} missing tags "
                f"(via {'/'.join(m['name'] for m in config['models'])})",
                repo_root, trailers=trailers,
            )
            log_fn(f"[{fmt}] FIXED: closed {closed} gaps (committed)")
            lesson("fixed", f"closed {closed} gap(s)", tag_key=_gap_primary_tag_key(gap))
            result = {"format": fmt, "status": "fixed", "gaps_closed": closed, "rounds": rounds}
            if review_flags:
                result["review_flags"] = review_flags
            return result

        log_fn(f"[{fmt}] review REJECTED: {review_reason}")
        git_checkout_clean_fn(repo_root)
        lesson("review_rejected", review_reason, checklist_id=parse_checklist_id(review_reason),
              tag_key=_gap_primary_tag_key(gap))
        rounds.append({"diff": diff, "reason": f"rejected by review: {review_reason}", "critique": review_reason})

        # Not every falsy verdict is a judgment. review_verdict returns
        # "review call failed: ..." when the reviewer ENDPOINT was down or
        # over a cost cap, and "unparseable review verdict: ..." when the
        # reply carried no usable verdict -- neither says anything about
        # the diff. They must not buy an extra round (a reviewer outage
        # would otherwise double every candidate's budget fleet-wide, each
        # extra round a full attempt_build + build + recheck that no
        # reviewer ever sees) and must not enter review_rejections (the
        # "still binding" handoff below would demand the fixer satisfy a
        # connection error with a diff). Same species as
        # INFRA_FAILURE_PREFIX on the build path.
        review_infra = review_reason.startswith(REVIEW_INFRA_PREFIXES)
        if not review_infra:
            review_rejections.append(review_reason)

            # The rejection buys one extra round, up to max_review_rounds --
            # BEFORE the last-round check below, so the round this rejection
            # pays for is a round that actually gets used. Doing it after
            # would extend a budget the function has already returned on.
            if review_extensions_used < max_review_rounds:
                review_extensions_used += 1
                rounds_allowed += 1
                log_fn(
                    f"[{fmt}] review round {len(review_rejections)}: "
                    f"{review_extensions_used}/{max_review_rounds} extra rounds used, "
                    f"{rounds_allowed - round_index - 1} left"
                )

        if round_index >= rounds_allowed - 1:
            return {
                "format": fmt, "status": "failed",
                "reason": f"rejected by review: {review_reason}", "diff": diff, "rounds": rounds,
            }

        if review_infra:
            # No judgment happened, so there is no objection to relay --
            # and provider noise must never reach a prompt (the ledger-side
            # twin of this rule is InfraNoiseNeverReachesAPromptTests).
            messages.append({
                "role": "user",
                "content": (
                    "The reviewer could not be reached for that diff, so it was not "
                    "judged on its merits. The working tree has been reverted, so your "
                    "next diff must apply to the ORIGINAL files, not to your previous "
                    "patch. Resend your fix -- improved if you can see a weakness, "
                    "otherwise as it was."
                ),
            })
            continue

        # Hand back the WHOLE argument so far, not just the latest turn of
        # it (see review_rejections). The older objections are numbered and
        # explicitly still binding, because the failure mode this replaces
        # is a fixer that treats each rejection as a fresh instruction and
        # un-fixes the previous one to satisfy it.
        if len(review_rejections) == 1:
            history = f"A reviewer rejected this fix: {review_reason}"
        else:
            prior = "\n".join(
                f"  {i}. {r}" for i, r in enumerate(review_rejections[:-1], start=1)
            )
            history = (
                f"A reviewer rejected this fix again (rejection {len(review_rejections)}): "
                f"{review_reason}\n\n"
                f"It also rejected your earlier attempts for these reasons:\n{prior}\n\n"
                "Every one of those is still binding. Your next diff must satisfy ALL of "
                "them at once -- fixing the newest objection by reintroducing something "
                "you were already rejected for is the most common way this loop is wasted."
            )
        messages.append({
            "role": "user",
            "content": (
                f"{history}\n\nThe working tree has been reverted, so your next diff must "
                "apply to the ORIGINAL files, not to your rejected patch. Investigation is "
                "still free: REQUEST any files you need to re-read before resending, as "
                "many as you want. Then send a corrected diff."
            ),
        })

    # Unreachable: the loop above always returns by its last iteration
    # (round_index >= rounds_allowed - 1 is covered by every branch, and
    # rounds_allowed only ever grows on a path that re-checks it).
    return {"format": fmt, "status": "failed", "reason": "exhausted repair rounds", "diff": diff, "rounds": rounds}


def run_loop(config, find_gaps_fn, fix_gap_fn, max_dry_rounds=2,
             git_checkout_clean_fn=None, repo_root=None):
    """Loop-until-dry driver. Returns a summary dict.

    A round is dry iff it closes zero gaps (not "discovers nothing new").
    A format that fails twice across rounds is skipped for the rest of
    the run.

    git_checkout_clean_fn/repo_root, if both given, are called right when a
    format hits its second failure and gets skip-listed -- belt-and-suspenders
    insurance on top of fix_gap's own per-attempt cleanup, so a format that's
    given up on can never leave dirty/untracked files (beyond gitignored
    build caches like target/, which checkout+clean never touches) behind
    for whatever gap gets attempted next.
    """
    skip_list = set()
    fail_counts = {}
    fixed, failed, skipped = [], [], []
    dry_rounds = 0
    round_num = 0

    while dry_rounds < max_dry_rounds:
        round_num += 1
        gaps = [g for g in find_gaps_fn() if g["format"] not in skip_list]
        if not gaps:
            dry_rounds += 1
            continue

        closed_this_round = 0
        for gap in gaps:
            result = fix_gap_fn(gap, config)
            if result["status"] == "fixed":
                fixed.append(result)
                closed_this_round += 1
            else:
                failed.append(result)
                fail_counts[gap["format"]] = fail_counts.get(gap["format"], 0) + 1
                if fail_counts[gap["format"]] >= 2:
                    skip_list.add(gap["format"])
                    skipped.append(gap["format"])
                    if git_checkout_clean_fn and repo_root:
                        git_checkout_clean_fn(repo_root)

        dry_rounds = 0 if closed_this_round else dry_rounds + 1

    return {
        "rounds": round_num,
        "fixed": fixed,
        "failed": failed,
        "skipped": sorted(set(skipped)),
    }


def tag_key_for(format_name, entry, kind):
    """Stable identity string for one tag within one format -- the
    persistent blacklist's dict key. kind is "missing" or "diff";
    value_differences entries already carry a combined "tag_key" like
    "EXIF:ISO", while missing_tags entries need family+name joined."""
    if kind == "diff":
        return f"{format_name}:{entry['tag_key']}"
    return f"{format_name}:{entry['family']}:{entry['name']}"


def expand_gaps_to_tags(gaps):
    """Flatten format-level gaps (as returned by find_gaps_fn) into one
    entry per individual tag, across every format -- the actual unit of
    work run_tag_loop attempts and blacklists, per-tag rather than
    per-format."""
    tag_gaps = []
    for g in gaps:
        fmt = g["format"]
        for t in g["missing_tags"]:
            tag_gaps.append({
                "format": fmt, "tag_key": tag_key_for(fmt, t, "missing"),
                "kind": "missing", "entry": t, "parser_files": g["parser_files"],
            })
        for d in g["value_differences"]:
            tag_gaps.append({
                "format": fmt, "tag_key": tag_key_for(fmt, d, "diff"),
                "kind": "diff", "entry": d, "parser_files": g["parser_files"],
            })
    return tag_gaps


def tag_still_open(match, tag_gap):
    """Is this one tag still a gap in a fresh comparison? Checks BOTH
    lists regardless of the tag's original kind: a kind=="missing" tag
    that a fix made present-but-wrong moves from missing_in_oxidex into
    value_differences -- counting only its original list called that
    "closed", which is exactly how a wrong-valued XMP:ArtworkTitle fix
    passed recheck and survived to human sweep review. Returns None
    (closed), ("missing",), ("duplicate_emission",), or
    ("value_differs", exiftool_value, oxidex_value) so the caller can put
    the actual values in front of the model on the retry.

    Spec M3 (double-emission gate): a target tag whose key appears in
    match["duplicate_emissions"] (an oxidex-side tag key the Rust
    ComparisonEngine found emitted more than once for the SAME sample
    file -- see comparison/engine.rs's compare()) is ALSO still open,
    checked FIRST and regardless of what the missing/value_differences
    lists would otherwise say: a fix that only "closes" the gap count by
    emitting a tag twice must never pass recheck."""
    if not match:
        return None
    if tag_gap["kind"] == "missing":
        fam, name = tag_gap["entry"]["family"], tag_gap["entry"]["name"]
        key = f"{fam}:{name}"
        if key in (match.get("duplicate_emissions") or []):
            return ("duplicate_emission",)
        if any(t.get("family") == fam and t.get("name") == name
               for t in match.get("missing_tags") or []):
            return ("missing",)
    else:
        key = tag_gap["entry"]["tag_key"]
        if key in (match.get("duplicate_emissions") or []):
            return ("duplicate_emission",)
    for d in match.get("value_differences") or []:
        if d.get("tag_key") == key:
            return ("value_differs", d.get("exiftool_value"), d.get("oxidex_value"))
    return None


def new_oxidex_only_keys(pre_report, post_report):
    """Spec M3: sorted "family:name" keys present in post_report's
    extra_in_oxidex but absent from pre_report's -- oxidex-side tags that
    appeared as a SIDE EFFECT of the change under test. pre_report/
    post_report are per-format comparison dicts carrying an
    "extra_in_oxidex" list of {"family", "name", ...} dicts (the same
    shape group_gaps_by_format threads straight from the Rust
    ComparisonReport, see find_tag_gaps.py). Lineage (same worktree, same
    tree modulo the change under test) is the caller's responsibility --
    this function is a pure set difference. Either side missing/falsy is
    "no extra tags" for that side."""
    def keys(report):
        return {
            f"{e.get('family')}:{e.get('name')}"
            for e in (report or {}).get("extra_in_oxidex") or []
        }
    return sorted(keys(post_report) - keys(pre_report))


def newly_duplicated_emissions(pre_report, post_report):
    """Spec M3 sibling: duplicate_emissions present in post but NOT in pre.

    The same pure set difference new_oxidex_only_keys performs, for the
    other half of the same gate -- and the half that was missing it.

    squad_merge_loop's post-merge check read `duplicate_emissions` straight
    off the POST report while diffing `extra_in_oxidex` properly, in one
    expression:

        dup = (post or {}).get("duplicate_emissions") or []      # post only
        introduced = new_oxidex_only_keys(pre, post)             # diffed
        if dup or introduced: ...quarantine...

    So any PRE-EXISTING duplicate quarantined every commit for that format,
    permanently, whatever the commit did. Measured 2026-07-27: NEF carries
    nine on clean main --

        EXIF:BitsPerSample, EXIF:Compression, EXIF:ImageHeight,
        EXIF:ImageWidth, EXIF:PhotometricInterpretation, EXIF:RowsPerStrip,
        EXIF:SamplesPerPixel, EXIF:StripOffsets, EXIF:SubfileType

    -- so NEF work could never be consumed, and the commit that tripped it
    (d8168e7b) had introduced none of them. This surfaced only once #135
    made duplicate detection work at all; before that the field was always
    empty and the missing diff could not bite.

    A commit is answerable for the duplicates it INTRODUCES, never for the
    ones it inherits.
    """
    def keys(report):
        return set((report or {}).get("duplicate_emissions") or [])
    return sorted(keys(post_report) - keys(pre_report))


DEFAULT_TABLE_PORT_THRESHOLD = 0.8


def evaluate_table_port_gate(pre_report, post_report, table_members, threshold=DEFAULT_TABLE_PORT_THRESHOLD):
    """T3 TABLE-PORT's THREE-CLAUSE acceptance gate (spec S3: "the
    critiqued two-clause gate shipped wrong values"). Pure function of
    two per-format comparison dicts (the same missing_tags/
    value_differences shape group_gaps_by_format/tag_still_open already
    use) and the table's member list (tag_key strings, "family:name"), so
    one call answers all three clauses -- and their interaction -- at
    once; there is no separate "close enough" check a caller could
    mistake for sufficient on its own.

    (a) At least `threshold` (default 80%) of table_members close EXACT
        in post_report: present in neither its missing_tags nor its
        value_differences.
    (b) Zero regressions: a member that was EXACT in pre_report must
        still be exact in post_report. Scoping choice: checked only
        across table_members, not the whole format -- the broader
        full-corpus regression concern is the targeted/workspace test
        gates and the M3 double-emission check the caller
        (attempt_table_port) already runs around this gate, so this
        function stays a pure, table-scoped check. A member that was
        already missing/wrong BEFORE the attempt moving to "wrong" is
        NOT a regression (nothing that was working broke) -- it's caught
        by clause (c) instead.
    (c) Zero members present-but-wrong: every post_report member found in
        value_differences is collected into `must_remove` -- spec S3's
        contract is that each of these must be commented out of the
        emission (a `// TODO(tag_key)` marker) before the commit lands,
        never shipped wrong. ANY non-empty must_remove fails the gate on
        its own, regardless of (a)/(b) -- there is no close-enough
        exception for a wrong value.

    Returns (passed: bool, reason: str, must_remove: list[str]).
    must_remove is always returned (even when passed, i.e. always []) so
    a caller can log/act on it uniformly either way.
    """
    def status_of(report, member):
        wrong_keys = {d.get("tag_key") for d in (report or {}).get("value_differences") or []}
        missing_keys = {
            f"{m.get('family')}:{m.get('name')}" for m in (report or {}).get("missing_tags") or []
        }
        if member in wrong_keys:
            return "wrong"
        if member in missing_keys:
            return "missing"
        return "exact"

    total = len(table_members)
    post_status = {m: status_of(post_report, m) for m in table_members}
    pre_status = {m: status_of(pre_report, m) for m in table_members}

    exact_count = sum(1 for s in post_status.values() if s == "exact")
    ratio = (exact_count / total) if total else 0.0

    regressions = [
        m for m in table_members
        if pre_status.get(m) == "exact" and post_status.get(m) != "exact"
    ]
    must_remove = [m for m in table_members if post_status.get(m) == "wrong"]

    reasons = []
    if total == 0:
        reasons.append("no table members given")
    elif ratio < threshold:
        reasons.append(
            f"only {exact_count}/{total} members exact ({ratio:.0%} < {threshold:.0%} threshold, clause a)"
        )
    if regressions:
        reasons.append(
            f"{len(regressions)} previously-exact member(s) regressed (clause b): {', '.join(regressions)}"
        )
    if must_remove:
        reasons.append(
            f"{len(must_remove)} member(s) present-but-wrong, must be removed/commented before commit "
            f"(clause c): {', '.join(must_remove)}"
        )

    passed = not reasons
    reason = (
        "; ".join(reasons) if reasons
        else f"{exact_count}/{total} members exact ({ratio:.0%}), zero regressions, zero present-but-wrong"
    )
    return passed, reason, must_remove


def make_single_tag_gap(tag_gap):
    """Build a synthetic single-tag "gap" dict with the same shape fix_gap/
    build_prompt already expect (format/missing_tags/value_differences/
    gap_count/parser_files), scoped to exactly the one tag in tag_gap.
    Reuses the existing single-shot-patch machinery unchanged -- gap_count
    is 1, so fix_gap's "did remaining decrease" check means "is this one
    tag still missing/differing", not a whole format's tally."""
    entry = tag_gap["entry"]
    return {
        "format": tag_gap["format"],
        "missing_tags": [entry] if tag_gap["kind"] == "missing" else [],
        "value_differences": [entry] if tag_gap["kind"] == "diff" else [],
        "gap_count": 1,
        "parser_files": tag_gap["parser_files"],
    }


DEFAULT_MAX_CLUSTER_TAGS = 6


def cluster_key(tag_gap):
    """Which tags belong in one conversation: same format, same family
    (middle component of the tag key -- EXIF, APP12, ZIP, ...), same
    parser files. Sibling tags in one table are usually one generalized
    branch away from each other (BlackLevelRed/Green/Blue, MODE1..6,
    ZipCRC/ZipCompressedSize all were), so investigating them separately
    re-pays the whole context cost per tag."""
    parts = tag_gap["tag_key"].split(":")
    family = parts[1] if len(parts) >= 3 else ""
    return (tag_gap["format"], family, tuple(tag_gap.get("parser_files") or ()))


def make_cluster_gap(leader):
    """make_single_tag_gap generalized to a leader plus its
    cluster_members: one gap dict whose tag lists union every member,
    gap_count == member count (so fix_gap's decrease check means "at
    least one of these closed"), and "clustered": True (so build_prompt
    shows every member even when max_prompt_tags is 1)."""
    members = [leader] + list(leader.get("cluster_members") or [])
    missing = [m["entry"] for m in members if m["kind"] == "missing"]
    diffs = [m["entry"] for m in members if m["kind"] == "diff"]
    return {
        "format": leader["format"],
        "missing_tags": missing,
        "value_differences": diffs,
        "gap_count": len(members),
        "parser_files": leader["parser_files"],
        "clustered": True,
    }


DEFAULT_LANDED_TAGS_PATH = OXIDEX_HOME / "logs" / "landed-tags.log"


def load_landed_tags(path):
    """tag_keys the sweep has already landed (see log_sweep_review.py's
    accepted-verdict append) -- workers skip these instead of re-deriving
    a fix that's already merged (observed live: the ZIP worker reproduced
    the identical ZipCRC diff a full round after the sweep landed it).
    Missing/corrupt file = empty set; each line is "<iso-ts> <tag_key>".

    Spec M5 tombstones: a "<iso-ts> REVERTED <tag_key>" line (appended by
    log_sweep_review.py --revert) REMOVES the tag from the landed set so
    a reverted fix's tag re-enters the worker pool instead of being
    suppressed forever. Lines replay in file order, so a re-land after a
    revert puts the tag back in the skip set."""
    try:
        text = Path(path).read_text()
    except OSError:
        return set()
    landed = set()
    for line in text.splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) != 2:
            continue
        rest = parts[1]
        if rest.startswith("REVERTED "):
            landed.discard(rest[len("REVERTED "):].strip())
        else:
            landed.add(rest)
    return landed


def load_tag_state(path):
    """Load the persistent per-tag blacklist/fail-count/claim state.

    A missing file means "nothing recorded yet" -> {}. A torn or
    corrupt file is NOT the same thing: this state carries every
    worker's claims and blacklist history, so treating a torn read as
    empty -- what this used to do -- means the very next save_tag_state
    silently wipes every other worker's entries. That exact wipe was
    observed live (a reader racing a non-atomic writer saw a truncated
    file). Parse failure now logs clearly and raises; save_tag_state's
    tempfile+os.replace writes make a torn file a real corruption signal
    worth stopping for, never a routine race to paper over.
    """
    path = Path(path)
    try:
        text = path.read_text()
    except FileNotFoundError:
        return {}
    try:
        state = json.loads(text)
        if not isinstance(state, dict):
            raise ValueError(f"tag state is {type(state).__name__}, not a dict")
    except (json.JSONDecodeError, ValueError) as e:
        print(
            f"FATAL: tag state at {path} is unreadable ({e}) -- refusing to "
            "treat it as empty, which would let the next save wipe every "
            "worker's claims/blacklist entries. Inspect or restore the file "
            "before restarting.",
            file=sys.stderr,
        )
        raise ValueError(f"corrupt tag state at {path}: {e}") from e
    return state


def save_tag_state(path, state):
    """Atomically persist the tag state: write to a NamedTemporaryFile in
    the same directory, then os.replace over the real path. Readers
    (load_tag_state, other workers, dashboards) can never observe a
    half-written file -- they see the old complete state or the new one,
    nothing in between."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    with tempfile.NamedTemporaryFile(
        "w", dir=path.parent, prefix=path.name + ".", suffix=".tmp", delete=False,
    ) as f:
        f.write(json.dumps(state, indent=2))
        tmp_name = f.name
    os.replace(tmp_name, path)


def _state_locked(path, mutate_fn, load_state_fn=load_tag_state, save_state_fn=save_tag_state):
    """Run mutate_fn(state) -> (new_state, result) under an exclusive
    flock on path's sibling .lock file, loading the state fresh inside
    the lock and saving it before releasing -- the tag-state twin of
    _governor_locked. Every read-modify-write of the shared tag state
    (claiming, recording results, blacklisting, the exhaustion reset,
    heartbeats) goes through here, so two workers can never interleave
    load/save and lose each other's updates. Critical sections are
    milliseconds; even 100 claimants is well under 10 acquisitions/sec.

    Unlike the governor (whose state is disposable bookkeeping and
    rebuilds permissively), a corrupt tag state propagates
    load_tag_state's raise -- see its docstring for why that must never
    be papered over."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    lock_path = path.with_suffix(".lock")
    with open(lock_path, "w") as lock_f:
        fcntl.flock(lock_f, fcntl.LOCK_EX)
        state = load_state_fn(path)
        new_state, result = mutate_fn(state)
        save_state_fn(path, new_state)
        return result


def release_worker_claims(state_path, worker_id):
    """Release every live tag/job claim still owned by ``worker_id``.

    Workers normally release claims in run_tag_loop's result-recording step,
    but a dispatcher shutdown can terminate a worker while its model/build
    attempt is still in flight.  Keep the entry (including failure history and
    tier attribution) and remove only the ownership fields, under the same
    flock used by every other tag-state mutation.
    """
    if not worker_id:
        return 0

    def mutate(state):
        released = 0
        for entry in state.values():
            if isinstance(entry, dict) and entry.get("claimed_by") == worker_id:
                entry.pop("claimed_by", None)
                entry.pop("claimed_at", None)
                released += 1
        return state, released

    return _state_locked(state_path, mutate)


@contextmanager
def worker_claim_lifecycle(state_path, worker_id, signal_module=signal):
    """Make SIGTERM unwind normally, then release this worker's claims.

    Python's default SIGTERM action exits immediately and skips ``finally``
    blocks.  The fleet sends SIGTERM to each worker process group during a
    graceful restart, so translate it to ``SystemExit`` while the tag loop is
    active.  That gives this context manager a chance to release only claims
    that are still owned by this exact worker before preserving the standard
    128+signal exit status.
    """
    previous_sigterm = None
    installed_handler = False

    if worker_id and threading.current_thread() is threading.main_thread():
        previous_sigterm = signal_module.getsignal(signal_module.SIGTERM)

        def handle_sigterm(signum, _frame):
            raise SystemExit(128 + signum)

        signal_module.signal(signal_module.SIGTERM, handle_sigterm)
        installed_handler = True

    try:
        yield
    finally:
        try:
            release_worker_claims(state_path, worker_id)
        finally:
            if installed_handler:
                signal_module.signal(signal_module.SIGTERM, previous_sigterm)


DEFAULT_MAX_TAG_FAILS = 10

# A claim's staleness threshold and the heartbeat that keeps it fresh.
# With a time-based heartbeat re-stamping claimed_at every
# DEFAULT_HEARTBEAT_SECONDS while an attempt is in flight, a stale claim
# now means "the owning process is dead", not merely "the owning process
# is slow" -- so the threshold is generous (2 h, up from the old 30 min,
# which real attempts overran routinely while queued behind the governor
# or a long cargo run, letting a second worker double-claim the tag).
DEFAULT_CLAIM_STALE_SECONDS = 7200
DEFAULT_HEARTBEAT_SECONDS = 60

# The ref every worker fast-forwards onto before each round, and again after
# it publishes. Defaulted, not None, because "refresh is available if you ask
# for it" is not a property a fleet can rely on: a live run went days with
# --base-ref unset, so refresh_worktree_fn was None and every worker kept
# measuring against whatever commit it happened to start on. 606 of 928
# tracked tags (65%) were already fixed and merged upstream while still being
# re-attempted, and 99 more were blacklisted off a stale binary's output.
# Staying current is the fleet's correctness property, so it is the default;
# --base-ref '' opts out for a genuinely standalone run.
DEFAULT_BASE_REF = "origin/main"

# Keep in sync with parallel_tag_fix_loop.py's own copy of this default --
# each worker (whether launched directly or via the parallel wrapper)
# should only ever hold one tag at a time unless config.toml says
# otherwise. See that module for the full rationale.
DEFAULT_MAX_TAGS_PER_PROCESS = 1


# ---------------------------------------------------------------------------
# Spec S4 item 5: canonical claim keys with cross-tier exclusion
# ---------------------------------------------------------------------------
#
# Claims in the shared tag-state have historically keyed purely by tag
# (T1/T2's own tag_key). T3 TABLE-PORT and T4 FOUNDATION-UNLOCK introduce
# two more claim shapes that must never silently double-work the same
# underlying ExifTool source: a table port in flight on %Canon::
# CameraSettings must exclude a T1 worker from independently wiring one of
# that table's member tags, and vice versa; a foundation job in flight on
# module FLIR must exclude any T1/T3 claim on that module's tags/tables
# while it's live. Every synthetic non-tag claim (a T3 table-port or T4
# foundation-job claim) is stored in the SAME tag-state dict, under a key
# that can never collide with a real "FMT:family:name" tag_key -- see
# TABLE_JOB_CLAIM_PREFIX/FOUNDATION_JOB_CLAIM_PREFIX below -- so
# claim_conflicts, gather_live_claims, and the ordinary tag-claim path all
# share one flock'd file and one _state_locked critical section, exactly
# as spec S4 item 5 requires ("checked inside the same mutate_fn").

#: Synthetic tag-state keys for T3/T4 claims -- deliberately containing a
#: literal "::" separator no real "FMT:family:name" tag_key can produce
#: (family/name/format tokens never contain "::" in any live gap), so a
#: synthetic key can never collide with, or be mistaken for, a real one.
TABLE_JOB_CLAIM_PREFIX = "__table_job::"
FOUNDATION_JOB_CLAIM_PREFIX = "__foundation_job::"


def table_job_claim_key(table_name):
    return f"{TABLE_JOB_CLAIM_PREFIX}{table_name}"


def foundation_job_claim_key(module_name):
    return f"{FOUNDATION_JOB_CLAIM_PREFIX}{module_name}"


#: Mirrors attribute_gaps.py's own UNKNOWN_MODULE constant (not imported
#: from there -- attribute_gaps.py is a standalone CLI script, and the
#: two modules don't otherwise share a dependency edge).
UNKNOWN_ATTRIBUTION_MODULE = "unknown"


def resolve_canonical_table(tag_key, attribution=None):
    """Spec S4 item 5: the canonical (module, canonical_table) for a T1/T2
    tag claim, resolved via the gap-attribution index (S1's
    gap-attribution.json / attribute_gaps.build_attribution) when
    available -- purely ADVISORY, never required. attribution is that
    document's already-parsed dict ({"tags": {"<FMT>:<family>:<name>":
    {"module", "table", ...}, ...}, ...}) or None.

    tag_key here is exactly run_tag_loop's own "FMT:family:name" /
    "FMT:tag_key" string (see tag_key_for) -- which is byte-identical to
    gap-attribution.json's own tag keys (attribute_gaps.py's
    `f"{fmt}:{family}:{name}"`), so no reformatting is needed to look one
    up in the other.

    Returns (module, canonical_table) or (None, None) when attribution is
    absent, the key isn't in it, or its module is the "unknown" bucket
    (spec S1: accepted advisory noise, never confident enough to exclude
    a real claim on). canonical_table is None when the table field is
    blank even though the module resolved (a real, if table-less,
    module match) -- module-level T4 exclusion can still apply even
    without a table-level T3 exclusion.
    """
    if not attribution:
        return None, None
    entry = (attribution.get("tags") or {}).get(tag_key)
    if not entry:
        return None, None
    module = entry.get("module")
    if not module or module == UNKNOWN_ATTRIBUTION_MODULE:
        return None, None
    table = entry.get("table") or ""
    canonical_table = f"{module}::{table}" if table else None
    return module, canonical_table


def claim_conflicts(existing_claims, new_claim):
    """Pure predicate (spec S4 item 5): does new_claim conflict with any
    entry in existing_claims? Both new_claim and every entry of
    existing_claims are dicts shaped:

        {"tier": "T1"|"T2"|"T3"|"T4", "tag_key": str or None,
         "canonical_table": str or None, "canonical_module": str or None}

    Truth table (every combination spec S4 item 5 calls out):
      - T1/T2 vs T1/T2, SAME tag_key -> conflict (pre-existing single-tag
        claim behavior -- kept here too so the whole cross-tier picture
        lives in one place, not regressed).
      - T1/T2 vs T1/T2, different tag_key -> no conflict, even when both
        happen to share a canonical_table (ordinary same-table T1
        grinding on different sibling tags is exactly normal, not
        excluded).
      - T1/T2 vs T3, or T3 vs T3, SAME canonical_table -> conflict (a
        table port in flight excludes per-tag work on its own members,
        and excludes a second concurrent port of the same table).
      - T1/T2 vs T3 / T3 vs T3, canonical_table absent on either side, or
        different -> no conflict (advisory attribution couldn't resolve
        a confident shared table, or they're genuinely different
        tables).
      - T4 vs ANYTHING (including another T4), SAME canonical_module ->
        conflict (a foundation job claims by module, per spec S3/S4 --
        excludes T1/T2/T3 claims on that module's tags/tables, and a
        second concurrent foundation job on the same module).
      - Different canonical_table / canonical_module (or either side
        None) -> no conflict.

    A tier not given defaults to "T1" (every pre-Phase-4 caller's claims
    are implicitly T1/T2 tag claims).
    """
    new_tier = new_claim.get("tier") or "T1"
    for existing in existing_claims:
        existing_tier = existing.get("tier") or "T1"

        if new_tier in ("T1", "T2") and existing_tier in ("T1", "T2"):
            if new_claim.get("tag_key") and new_claim.get("tag_key") == existing.get("tag_key"):
                return True
            continue

        if new_tier == "T4" or existing_tier == "T4":
            new_module = new_claim.get("canonical_module")
            existing_module = existing.get("canonical_module")
            if new_module and new_module == existing_module:
                return True
            continue

        # Every remaining combination involves T3 on at least one side
        # (T1/T2 vs T3, or T3 vs T3) -- table-scoped exclusion.
        new_table = new_claim.get("canonical_table")
        existing_table = existing.get("canonical_table")
        if new_table and new_table == existing_table:
            return True

    return False


def gather_live_claims(state, time_fn=time.time, claim_stale_seconds=DEFAULT_CLAIM_STALE_SECONDS,
                        exclude_key=None):
    """Every currently-live (claimed, not stale) entry in the shared
    tag-state, rendered as claim_conflicts-shaped dicts -- covers ordinary
    T1/T2 tag entries AND synthetic T3/T4 job entries (see
    table_job_claim_key/foundation_job_claim_key) uniformly, since both
    live in the same state dict under the same "claimed_by"/"claimed_at"
    convention. exclude_key, if given, omits that one key (typically the
    entry being claimed/renewed itself)."""
    now = time_fn()
    claims = []
    for key, entry in state.items():
        if key == exclude_key or not isinstance(entry, dict):
            continue
        claimed_by = entry.get("claimed_by")
        if not claimed_by:
            continue
        claimed_at = entry.get("claimed_at", 0)
        if now - claimed_at >= claim_stale_seconds:
            continue
        tier = entry.get("tier") or "T1"
        claims.append({
            "tier": tier,
            "tag_key": key if tier in ("T1", "T2") else None,
            "canonical_table": entry.get("canonical_table"),
            "canonical_module": entry.get("canonical_module"),
        })
    return claims


def claim_table_job(state_path, table_name, module, worker_id,
                     claim_stale_seconds=DEFAULT_CLAIM_STALE_SECONDS, time_fn=time.time):
    """Claim a T3 TABLE-PORT job's synthetic tag-state entry (spec S4
    item 5), refusing (returns False) if a live claim anywhere in the
    shared state conflicts (claim_conflicts) -- a live T1/T2 claim on one
    of this table's own members, a live T3 claim already porting the
    same table, or a live T4 claim on the same module. Renewing an
    already-held claim (same worker_id) always succeeds and just
    refreshes claimed_at. Runs inside ONE _state_locked critical section,
    exactly like every other claim in this fleet."""
    key = table_job_claim_key(table_name)
    canonical_table = f"{module}::{table_name.split('::')[-1]}" if module and table_name else None

    def mutate(state):
        existing = state.get(key)
        if isinstance(existing, dict) and existing.get("claimed_by") == worker_id:
            existing["claimed_at"] = time_fn()
            return state, True
        candidate = {"tier": "T3", "tag_key": None, "canonical_table": canonical_table, "canonical_module": module}
        live = gather_live_claims(state, time_fn, claim_stale_seconds, exclude_key=key)
        if claim_conflicts(live, candidate):
            return state, False
        state[key] = {
            "tier": "T3", "claimed_by": worker_id, "claimed_at": time_fn(),
            "canonical_table": canonical_table, "canonical_module": module,
            "table_name": table_name, "fails": 0, "blacklisted": False, "attempts": [],
        }
        return state, True

    return _state_locked(state_path, mutate)


def release_table_job_claim(state_path, table_name):
    """Release (but do not delete -- keeps fails/attempts history) a T3
    job claim's claimed_by/claimed_at, same convention run_tag_loop's own
    `record` step uses for an ordinary tag claim."""
    key = table_job_claim_key(table_name)

    def mutate(state):
        entry = state.get(key)
        if isinstance(entry, dict):
            entry.pop("claimed_by", None)
            entry.pop("claimed_at", None)
        return state, None

    _state_locked(state_path, mutate)


def claim_foundation_job(state_path, job, worker_id,
                          claim_stale_seconds=DEFAULT_CLAIM_STALE_SECONDS, time_fn=time.time):
    """Claim a T4 FOUNDATION-UNLOCK job's synthetic tag-state entry (spec
    S4 item 5) -- claims by MODULE (job["target_module"]), refusing if a
    live T1/T2/T3 claim anywhere shares that module, or another T4 claim
    is already live on it. Same renew/refuse/lock-scoping contract as
    claim_table_job."""
    module = job.get("target_module")
    key = foundation_job_claim_key(module or job["name"])

    def mutate(state):
        existing = state.get(key)
        if isinstance(existing, dict) and existing.get("claimed_by") == worker_id:
            existing["claimed_at"] = time_fn()
            return state, True
        candidate = {"tier": "T4", "tag_key": None, "canonical_table": None, "canonical_module": module}
        live = gather_live_claims(state, time_fn, claim_stale_seconds, exclude_key=key)
        if claim_conflicts(live, candidate):
            return state, False
        state[key] = {
            "tier": "T4", "claimed_by": worker_id, "claimed_at": time_fn(),
            "canonical_module": module, "job_name": job["name"],
            "fails": 0, "blacklisted": False, "attempts": [],
        }
        return state, True

    return _state_locked(state_path, mutate)


def release_foundation_job_claim(state_path, job):
    module = job.get("target_module")
    key = foundation_job_claim_key(module or job["name"])

    def mutate(state):
        entry = state.get(key)
        if isinstance(entry, dict):
            entry.pop("claimed_by", None)
            entry.pop("claimed_at", None)
        return state, None

    _state_locked(state_path, mutate)


# =============================================================================
# T4 FOUNDATION-UNLOCK / T3 TABLE-PORT job mechanics (spec S3)
# =============================================================================
#
# Both tiers are fix_gap-like attempts scoped to a table/module rather than
# one tag: they reuse fix_gap's own core loop shape (attempt_build_fn for
# the build/repair conversation, review_fn for genuineness, cargo_test_*_fn
# for the test gates) and the same Perl-reference/neighbor-precedent prompt
# machinery build_prompt already uses -- see _foundation_job_pseudo_gap/
# _table_port_pseudo_gap, which synthesize a gap-shaped dict purely so
# build_perl_reference_block/build_neighbor_precedent_block can be called
# unmodified.

DEFAULT_FOUNDATION_JOBS_PATH = Path(__file__).resolve().parent / "foundation_jobs.toml"
_REQUIRED_FOUNDATION_JOB_FIELDS = ("name", "description", "target_formats", "target_module", "estimated_gaps")


def load_foundation_jobs(path=DEFAULT_FOUNDATION_JOBS_PATH):
    """Parse scripts/foundation_jobs.toml (spec S3's 7 human-curated T4
    seeds) into a list of plain dicts, one per `[[jobs]]` entry, each with
    "status" defaulted to "pending" when the TOML omits it.

    Pure data load -- validates only that every required field
    (name/description/target_formats/target_module/estimated_gaps) is
    present; the TOML file itself is the source of truth (hand-curated,
    checked in), not something this loader should second-guess the
    content of."""
    with open(path, "rb") as f:
        data = tomllib.load(f)
    jobs = []
    for entry in data.get("jobs") or []:
        missing = [field for field in _REQUIRED_FOUNDATION_JOB_FIELDS if field not in entry]
        if missing:
            raise ValueError(
                f"foundation job {entry.get('name', '?')!r} in {path} is missing required "
                f"field(s): {missing}"
            )
        job = dict(entry)
        job.setdefault("status", "pending")
        jobs.append(job)
    return jobs


def _foundation_job_pseudo_gap(job):
    """A gap-shaped dict (format/missing_tags/value_differences/
    parser_files/gap_count), synthesized from one foundation-job spec
    purely so the existing per-tag Perl-reference/neighbor-precedent
    machinery (build_perl_reference_block/build_neighbor_precedent_block)
    can be reused as-is for a job that targets a whole module/dispatch
    path rather than one tag -- spec S3 item 2(a): "reuse ... do not
    reimplement".

    job.get("tag_hints") -- an OPTIONAL list of {"name", "tag_id"} dicts a
    job entry MAY carry -- seeds missing_tags when given; the 7 checked-in
    seeds in foundation_jobs.toml don't carry this (they're dispatch/
    parser-scoped fixes with no single representative tag), so both
    downstream blocks simply come back empty for them -- the same
    "no match found" degradation these functions already handle for an
    ordinary per-tag gap, not a new failure mode."""
    formats = job.get("target_formats") or []
    module = job.get("target_module") or ""
    return {
        "format": formats[0] if formats else (module or job.get("name", "?")),
        "missing_tags": [
            {"name": h.get("name"), "tag_id": h.get("tag_id"), "family": module}
            for h in (job.get("tag_hints") or [])
        ],
        "value_differences": [],
        "parser_files": job.get("parser_files") or [],
        "gap_count": job.get("estimated_gaps") or 0,
    }


def build_foundation_job_prompt(job, perl_lib_dir=None, neighbor_precedent_block="",
                                 max_prompt_tokens=DEFAULT_TABLE_JOB_MAX_PROMPT_TOKENS,
                                 max_reply_tokens=DEFAULT_MAX_TOKENS):
    """T4 FOUNDATION-UNLOCK prompt (spec S3 item 2(a)). A foundation job
    targets a whole table/module dispatch path -- e.g. "CR3 QuickTime-box
    -> Canon CMT dispatch" -- rather than one tag, so there is no
    missing_tags/value_differences list to show the way build_prompt does
    for an ordinary gap; the job's own curated name/description/
    target_formats/target_module/estimated_gaps (see load_foundation_jobs)
    IS the spec instead. Reuses build_perl_reference_block/
    build_neighbor_precedent_block via _foundation_job_pseudo_gap."""
    gap = _foundation_job_pseudo_gap(job)
    perl_block = build_perl_reference_block(gap, perl_lib_dir) if perl_lib_dir else ""
    manifest = build_reply_shape_manifest(max_prompt_tokens, max_reply_tokens)
    targets = ", ".join(job.get("target_formats") or [job.get("target_module") or "?"])
    sections = [
        ("intro", (
            f"{RUST_ARCHITECTURE_CONSTRAINTS}\n\n"
            "You are landing a T4 FOUNDATION-UNLOCK job in the oxidex Rust codebase: a "
            "table/module-scoped dispatch or parser fix that unlocks MANY downstream "
            "per-tag gaps at once, not a single tag.\n\n"
            f"Job: {job['name']}\n"
            f"Target format(s): {targets}\n"
            f"ExifTool module unlocked: {job.get('target_module') or '?'}\n"
            f"Estimated gaps this unlocks (advisory, from the gap census): {job.get('estimated_gaps', '?')}\n\n"
            f"Description:\n{job['description'].strip()}\n\n"
            f"{KNOWN_PITFALLS}\n\n{manifest}\n\n"
        )),
        ("perl_block", perl_block),
        ("neighbor", neighbor_precedent_block),
        ("tail", (
            "\n\nThis is dispatch/parser plumbing, not per-tag value wiring -- land the "
            "walker/dispatch logic and wire it to EXISTING tag-parsing code wherever "
            "possible (see the pitfalls above); do not reimplement a manufacturer's tag "
            "table from scratch if a working parser for it already exists elsewhere in "
            f"this codebase.\n\n{TERMINAL_REMINDER}"
        )),
    ]
    budgets = {"perl_block": 0, "neighbor": 0}
    return assemble_prompt_sections(sections, budgets, max_prompt_tokens)


def resolve_foundation_job_tag_keys(job, tag_state, attribution=None):
    """Best-effort (spec S3): which tag_keys in the shared tag-state does
    landing `job` plausibly unlock? Confident match only -- "err toward
    not tagging rather than mis-tagging" (spec).

    Two independent, either-sufficient signals:
      1. The tag-state entry's OWN canonical_module (stamped at claim
         time by spec S4 item 5's resolution -- see run_tag_loop's
         claim() closure) equals job["target_module"].
      2. attribution (gap-attribution.json), if given: resolve_canonical_table
         on the tag_key itself resolves to job["target_module"].

    Synthetic T3/T4 job-claim keys (table_job_claim_key/
    foundation_job_claim_key) are never matched -- they aren't real tags.
    Returns a sorted list; empty is a valid, expected outcome when
    nothing confidently matches (e.g. no worker has ever touched a tag
    under this module, or attribution is unavailable)."""
    target_module = job.get("target_module")
    if not target_module:
        return []

    def is_real_tag_key(key):
        return not (key.startswith(TABLE_JOB_CLAIM_PREFIX) or key.startswith(FOUNDATION_JOB_CLAIM_PREFIX))

    matched = set()
    for tag_key, entry in (tag_state or {}).items():
        if not is_real_tag_key(tag_key) or not isinstance(entry, dict):
            continue
        if entry.get("canonical_module") == target_module:
            matched.add(tag_key)
        elif attribution:
            module, _ = resolve_canonical_table(tag_key, attribution)
            if module == target_module:
                matched.add(tag_key)
    return sorted(matched)


def mark_held_by_foundation(state_path, job, commit_sha, attribution=None,
                            load_state_fn=load_tag_state, save_state_fn=save_tag_state):
    """Spec S3 T4: stamp held_by_foundation={"job": <name>, "sha": <sha>}
    (the EXACT minimal shape parallel_model_fix_loop.clear_held_by_foundation,
    Phase 3, already knows how to clear once <sha> reaches origin/main)
    onto every tag_key resolve_foundation_job_tag_keys confidently matches
    to this job's module -- resolved FRESH inside the same locked
    read-modify-write (not a separately-read snapshot passed in), so
    there's no read-then-mark race window against a concurrent claim on
    this exact module.

    A tag_key not currently present in tag-state is simply skipped -- this
    never CREATES a placeholder entry (a foundation job landing doesn't
    itself discover new gaps; the next find_gaps_fn round does that).
    Returns the list of tag_keys actually stamped."""
    def mutate(state):
        matching = resolve_foundation_job_tag_keys(job, state, attribution)
        stamped = []
        for tag_key in matching:
            entry = state.get(tag_key)
            if not isinstance(entry, dict):
                continue
            entry["held_by_foundation"] = {"job": job["name"], "sha": commit_sha}
            stamped.append(tag_key)
        return state, stamped

    return _state_locked(state_path, mutate, load_state_fn, save_state_fn)


def attempt_foundation_job(job, repo_root, config, *, call_model_fn=call_model,
                           review_call_model_fn=None, critique_call_model_fn=None,
                           review_config=None,
                           git_apply_fn=git_apply, git_checkout_clean_fn=git_checkout_clean,
                           git_commit_fn=git_commit, cargo_build_fn=cargo_build,
                           cargo_test_workspace_fn=cargo_test_workspace,
                           cargo_test_targeted_fn=cargo_test_targeted, cargo_check_fn=cargo_check,
                           attempt_build_fn=attempt_build, review_fn=review_verdict,
                           critique_fn=critique_failed_attempt, pick_model_fn=random.choice,
                           log_fn=print, perl_lib_dir=None, worker_label=None,
                           neighbor_precedent_block="", table_job_config=None,
                           state_path=None, attribution=None, git_rev_parse_fn=None):
    """T4 FOUNDATION-UNLOCK: attempt ONE foundation job end to end (spec
    S3 item 2) -- a fix_gap-like attempt (build/repair conversation via
    attempt_build_fn, genuineness review via review_fn, targeted +
    workspace test gates) scoped to a table/module dispatch path rather
    than one tag, using build_foundation_job_prompt instead of
    build_prompt.

    table_job_config, if given, is the parsed `[table_job]` section (see
    normalize_table_job_config) -- max_prompt_tokens/max_repair_rounds;
    defaults (DEFAULT_TABLE_JOB_MAX_PROMPT_TOKENS/
    DEFAULT_TABLE_JOB_MAX_REPAIR_ROUNDS) otherwise. The model pool is
    narrowed to phase="table" (spec: "strongest configured model") via
    models_for_phase before any attempt_build_fn call -- attempt_build_fn
    itself is used completely unmodified; only the config handed to it
    differs from an ordinary per-tag fix_gap call.

    Unlike attempt_table_port's three-clause acceptance gate (there is no
    well-defined "table membership" for a dispatch/parser-plumbing job),
    the acceptance bar here is simply: builds, passes the targeted +
    workspace test suites, and the reviewer approves -- exactly fix_gap's
    own bar minus its per-tag gap-count recheck (which has no foundation-
    job analogue).

    On landing a commit: if state_path is given, mark_held_by_foundation
    stamps held_by_foundation on every tag_key resolve_foundation_job_tag_keys
    confidently matches (best-effort, see that function's own contract);
    None (the default) skips this -- a caller not tracking tag-state
    (e.g. a dry-run/manual invocation) is unaffected.

    Returns a result dict: {"job": name, "status": "fixed"|"failed",
    "rounds": [...], plus "commit_sha"/"held_tags" on success or
    "reason" on failure} -- deliberately NOT fix_gap's own {"format": ...}
    shape (there is no single format this job "is").
    """
    review_config = review_config or config
    table_job_config = table_job_config or normalize_table_job_config({})
    job_config = dict(config)
    job_config["models"] = models_for_phase(config["models"], "table")
    job_config["max_prompt_tokens"] = table_job_config.get(
        "max_prompt_tokens", DEFAULT_TABLE_JOB_MAX_PROMPT_TOKENS)
    max_repair_rounds = table_job_config.get("max_repair_rounds", DEFAULT_TABLE_JOB_MAX_REPAIR_ROUNDS)
    git_rev_parse_fn = git_rev_parse_fn or (lambda rr: _run_git(["rev-parse", "HEAD"], rr).strip())

    pseudo_gap = _foundation_job_pseudo_gap(job)
    messages = [{"role": "user", "content": build_foundation_job_prompt(
        job, perl_lib_dir=perl_lib_dir, neighbor_precedent_block=neighbor_precedent_block,
        max_prompt_tokens=job_config["max_prompt_tokens"],
        max_reply_tokens=job_config.get("max_tokens", DEFAULT_MAX_TOKENS),
    )}]

    rounds = []
    diff = None
    targeted_filter = (job.get("target_formats") or [job.get("target_module") or job["name"]])[0].lower()

    def critique_and_continue(failure_kind, reason, round_index):
        if reason.startswith(INFRA_FAILURE_PREFIX):
            critique = reason
        else:
            critique = critique_fn(
                pseudo_gap, diff, failure_kind, reason, job_config,
                call_model_fn=critique_call_model_fn or call_model_fn, pick_model_fn=pick_model_fn,
            )
        rounds.append({"diff": diff, "reason": reason, "critique": critique})
        if round_index == max_repair_rounds - 1:
            return {"job": job["name"], "status": "failed", "reason": reason, "diff": diff, "rounds": rounds}
        messages.append({
            "role": "user",
            "content": (
                f"That attempt failed ({failure_kind}): {reason}\n\n"
                f"Reviewer critique: {critique}\n\nPlease resend a corrected diff."
            ),
        })
        return None

    for round_index in range(max_repair_rounds):
        built, reason, diff, messages = attempt_build_fn(
            messages, call_model_fn=call_model_fn, git_apply_fn=git_apply_fn,
            git_checkout_clean_fn=git_checkout_clean_fn, cargo_build_fn=cargo_build_fn,
            config=job_config, repo_root=repo_root, pick_model_fn=pick_model_fn,
            cargo_check_fn=cargo_check_fn,
        )
        if not built:
            log_fn(f"[foundation:{job['name']}] build failed: {reason}")
            outcome = critique_and_continue("build_failed", reason, round_index)
            if outcome:
                return outcome
            continue

        t_ok, t_out = cargo_test_targeted_fn(repo_root, targeted_filter)
        if not t_ok:
            git_checkout_clean_fn(repo_root)
            reason = f"targeted tests ({targeted_filter}) regressed:\n{_extract_test_failure_context(t_out)}"
            log_fn(f"[foundation:{job['name']}] targeted tests regressed, reverting")
            outcome = critique_and_continue("test_regressed", reason, round_index)
            if outcome:
                return outcome
            continue

        approved, review_reason = review_fn(
            pseudo_gap, diff, review_config, call_model_fn=review_call_model_fn or call_model_fn,
            pick_model_fn=pick_model_fn,
        )
        if not approved:
            log_fn(f"[foundation:{job['name']}] review REJECTED: {review_reason}")
            git_checkout_clean_fn(repo_root)
            rounds.append({"diff": diff, "reason": f"rejected by review: {review_reason}", "critique": review_reason})
            if round_index == max_repair_rounds - 1:
                return {
                    "job": job["name"], "status": "failed",
                    "reason": f"rejected by review: {review_reason}", "diff": diff, "rounds": rounds,
                }
            messages.append({
                "role": "user",
                "content": f"A reviewer rejected this fix: {review_reason}\nPlease resend a corrected diff.",
            })
            continue

        tests_passed, test_output = cargo_test_workspace_fn(repo_root)
        if not tests_passed:
            git_checkout_clean_fn(repo_root)
            reason = f"cargo test --workspace regressed:\n{_extract_test_failure_context(test_output)}"
            log_fn(f"[foundation:{job['name']}] cargo test --workspace regressed, reverting")
            outcome = critique_and_continue("test_regressed", reason, round_index)
            if outcome:
                return outcome
            continue

        trailers = {
            "Format": ",".join(job.get("target_formats") or []),
            "Job": job["name"],
            "Worker": worker_label,
        }
        git_commit_fn(
            f"feat({(job.get('target_module') or job['name']).lower()}): foundation-unlock {job['name']}",
            repo_root, trailers=trailers,
        )
        commit_sha = git_rev_parse_fn(repo_root)
        log_fn(f"[foundation:{job['name']}] FOUNDATION LANDED (commit {commit_sha[:12] if commit_sha else '?'})")

        held_tags = []
        if state_path is not None and commit_sha:
            held_tags = mark_held_by_foundation(state_path, job, commit_sha, attribution)

        return {
            "job": job["name"], "status": "fixed", "commit_sha": commit_sha,
            "held_tags": held_tags, "rounds": rounds,
        }

    return {"job": job["name"], "status": "failed", "reason": "exhausted repair rounds", "diff": diff, "rounds": rounds}


def _table_port_pseudo_gap(table_name, module, repo_root, table_members=None):
    """A gap-shaped dict for a T3 TABLE-PORT job, mirroring
    _foundation_job_pseudo_gap -- reuses build_neighbor_precedent_block's
    registry-file precedent lookup. parser_files points at the module's
    own registries/<module>.rs (spec S3 item 3(iii): "registry precedent
    ... scoped to the target registry file").

    table_members (the table's own "family:name" tag_key membership --
    same shape evaluate_table_port_gate consumes), when given, seeds
    missing_tags -- exactly like _foundation_job_pseudo_gap's own
    tag_hints seeding. Without this, missing_tags/value_differences would
    stay permanently empty (there IS no per-tag gap here; this is a
    whole-table port), which leaves find_implemented_sibling's `families`
    search set empty by construction -- its `for family in
    sorted(families)` loop would then never execute and the sibling
    search would never run at all, regardless of what the registry file
    actually contains. own (derived from these same missing_tags) then
    excludes every member of THIS table, so the search only ever
    surfaces a DIFFERENT, already-implemented same-family tag as
    precedent, never one of the table's own (not-yet-ported) members."""
    short_table = table_name.split("::")[-1] if table_name else table_name
    registry_rel = str(REGISTRIES_RELATIVE_DIR / f"{(module or '').lower()}.rs")
    missing_tags = []
    for tag_key in (table_members or []):
        family, sep, name = tag_key.partition(":")
        if sep:
            missing_tags.append({"family": family, "name": name})
    return {
        "format": module or table_name or "?",
        "missing_tags": missing_tags,
        "value_differences": [],
        "parser_files": [registry_rel],
        "gap_count": 0,
        "_short_table": short_table,
    }


def build_table_port_prompt(table_name, module, perl_table_source, registry_skeleton,
                            neighbor_precedent_block="",
                            max_prompt_tokens=DEFAULT_TABLE_JOB_MAX_PROMPT_TOKENS,
                            max_reply_tokens=DEFAULT_MAX_TOKENS):
    """T3 TABLE-PORT prompt (spec S3 item 3): the FULL Perl table source
    (extract_perl_table_source -- ground truth), the oxidex-side
    id-to-name skeleton (build_table_port_registry_skeleton --
    SCAFFOLDING ONLY, labelled unambiguously as such and never as value
    ground truth), and registry precedent (neighbor_precedent_block,
    pre-rendered by the caller -- see attempt_table_port)."""
    manifest = build_reply_shape_manifest(max_prompt_tokens, max_reply_tokens)
    perl_section = (
        f"\n\nExifTool's own COMPLETE table source (ground truth -- port this table's "
        f"full membership, not a guess at it):\n\n{perl_table_source}"
        if perl_table_source else "\n\n(ExifTool table source unavailable -- work from the description below.)"
    )
    skeleton_section = (
        f"\n\noxidex's existing id-to-name registry for this table -- SCAFFOLDING ONLY, "
        "STRUCTURE, NOT VALUE GROUND TRUTH (the Perl table above is the ground truth; this "
        f"just shows the shape oxidex code already expects):\n\n{registry_skeleton}"
        if registry_skeleton else ""
    )
    sections = [
        ("intro", (
            f"{RUST_ARCHITECTURE_CONSTRAINTS}\n\n"
            "You are performing a T3 TABLE-PORT in the oxidex Rust codebase: port an "
            f"ENTIRE ExifTool %table -- Image::ExifTool::{table_name} (module {module}) -- "
            "not a single tag.\n\n"
            "ACCEPTANCE IS STRICT: (a) at least 80% of this table's members must close with "
            "EXACT values, not just be present; (b) zero regressions of previously-matching "
            "tags; (c) ZERO members may be present-but-wrong -- any member you cannot make "
            "exact must be REMOVED from emission (commented out, with a `// TODO(tag_key)` "
            "marker naming the tag) before your diff is final. A member that is simply "
            "absent is fine; a member that is emitted with a WRONG value is never "
            "acceptable and will fail review even if most other members are correct.\n\n"
            f"{KNOWN_PITFALLS}\n\n{manifest}\n\n"
        )),
        ("perl_table", perl_section),
        ("skeleton", skeleton_section),
        ("neighbor", neighbor_precedent_block),
        ("tail", f"\n\n{TERMINAL_REMINDER}"),
    ]
    budgets = {"neighbor": 0, "skeleton": 0, "perl_table": DEFAULT_PARSER_FLOOR_TOKENS}
    return assemble_prompt_sections(sections, budgets, max_prompt_tokens)


def attempt_table_port(table_name, module, repo_root, config, *, call_model_fn=call_model,
                       review_call_model_fn=None, critique_call_model_fn=None, review_config=None,
                       git_apply_fn=git_apply, git_checkout_clean_fn=git_checkout_clean,
                       git_commit_fn=git_commit, cargo_build_fn=cargo_build,
                       cargo_test_workspace_fn=cargo_test_workspace,
                       cargo_test_targeted_fn=cargo_test_targeted, cargo_check_fn=cargo_check,
                       attempt_build_fn=attempt_build, review_fn=review_verdict,
                       critique_fn=critique_failed_attempt, pick_model_fn=random.choice,
                       log_fn=print, perl_lib_dir=None, worker_label=None,
                       table_job_config=None, table_members=None, format_name=None,
                       pre_report=None, recheck_fn=None, threshold=DEFAULT_TABLE_PORT_THRESHOLD,
                       max_repair_rounds=None):
    """T3 TABLE-PORT: attempt to port ONE ExifTool %table end to end
    (spec S3 item 3) -- one job, one commit, exactly like fix_gap's own
    "one job, one commit, one process exit" invariant (S5), but scoped
    to every member of `table_name` rather than one tag.

    table_name is the table's name after "Image::ExifTool::" (e.g.
    "Canon::CameraSettings"); module is its owning ExifTool module (e.g.
    "Canon", used for the registry-skeleton lookup and the commit's
    Table:/Job-style trailers).

    table_members must be given: the list of tag_key strings ("family:name")
    this table's membership comprises (typically resolved via
    gap-attribution.json's per-tag "table" field, or a hand-curated list
    for a pilot table) -- required by evaluate_table_port_gate, which is
    the sole acceptance authority here (see below); an empty list always
    fails clause (a) (see that function's own "no table members given"
    handling) rather than silently skipping the gate.

    pre_report/recheck_fn implement spec M3's pre/post comparison pattern
    exactly like fix_gap's own recheck_baseline/recheck_fn: pre_report is
    the per-format comparison dict BEFORE this attempt; recheck_fn(format_name)
    is called after each round's build to get a FRESH one. Both are fed to
    evaluate_table_port_gate (not a simple gap-count check) -- the THREE-CLAUSE
    gate is the acceptance authority, not "did the count go down".

    On a round whose gate fails ONLY on clause (c) (must_remove non-empty,
    clauses a/b otherwise fine), the critique explicitly names the
    present-but-wrong members and instructs the fixer to comment them out
    with a `// TODO(tag_key)` marker -- exactly spec S3's own remediation
    -- rather than a generic "try again".
    """
    review_config = review_config or config
    table_job_config = table_job_config or normalize_table_job_config({})
    job_config = dict(config)
    job_config["models"] = models_for_phase(config["models"], "table")
    job_config["max_prompt_tokens"] = table_job_config.get(
        "max_prompt_tokens", DEFAULT_TABLE_JOB_MAX_PROMPT_TOKENS)
    max_repair_rounds = max_repair_rounds or table_job_config.get(
        "max_repair_rounds", DEFAULT_TABLE_JOB_MAX_REPAIR_ROUNDS)
    table_members = table_members or []
    format_name = format_name or module

    perl_table_source = extract_perl_table_source(table_name, perl_lib_dir) if perl_lib_dir else None
    registry_skeleton = build_table_port_registry_skeleton(module, table_name, repo_root)
    pseudo_gap = _table_port_pseudo_gap(table_name, module, repo_root, table_members=table_members)
    neighbor_precedent_block = build_neighbor_precedent_block(pseudo_gap, repo_root)

    messages = [{"role": "user", "content": build_table_port_prompt(
        table_name, module, perl_table_source, registry_skeleton,
        neighbor_precedent_block=neighbor_precedent_block,
        max_prompt_tokens=job_config["max_prompt_tokens"],
        max_reply_tokens=job_config.get("max_tokens", DEFAULT_MAX_TOKENS),
    )}]

    rounds = []
    diff = None
    targeted_filter = (format_name or table_name).lower()

    def critique_and_continue(failure_kind, reason, round_index):
        if reason.startswith(INFRA_FAILURE_PREFIX):
            critique = reason
        else:
            critique = critique_fn(
                pseudo_gap, diff, failure_kind, reason, job_config,
                call_model_fn=critique_call_model_fn or call_model_fn, pick_model_fn=pick_model_fn,
            )
        rounds.append({"diff": diff, "reason": reason, "critique": critique})
        if round_index == max_repair_rounds - 1:
            return {
                "table": table_name, "module": module, "status": "failed",
                "reason": reason, "diff": diff, "rounds": rounds,
            }
        messages.append({
            "role": "user",
            "content": (
                f"That attempt failed ({failure_kind}): {reason}\n\n"
                f"Reviewer critique: {critique}\n\nPlease resend a corrected diff."
            ),
        })
        return None

    for round_index in range(max_repair_rounds):
        built, reason, diff, messages = attempt_build_fn(
            messages, call_model_fn=call_model_fn, git_apply_fn=git_apply_fn,
            git_checkout_clean_fn=git_checkout_clean_fn, cargo_build_fn=cargo_build_fn,
            config=job_config, repo_root=repo_root, pick_model_fn=pick_model_fn,
            cargo_check_fn=cargo_check_fn,
        )
        if not built:
            log_fn(f"[table-port:{table_name}] build failed: {reason}")
            outcome = critique_and_continue("build_failed", reason, round_index)
            if outcome:
                return outcome
            continue

        t_ok, t_out = cargo_test_targeted_fn(repo_root, targeted_filter)
        if not t_ok:
            git_checkout_clean_fn(repo_root)
            reason = f"targeted tests ({targeted_filter}) regressed:\n{_extract_test_failure_context(t_out)}"
            log_fn(f"[table-port:{table_name}] targeted tests regressed, reverting")
            outcome = critique_and_continue("test_regressed", reason, round_index)
            if outcome:
                return outcome
            continue

        post_report = recheck_fn(format_name) if recheck_fn else None
        gate_passed, gate_reason, must_remove = evaluate_table_port_gate(
            pre_report, post_report, table_members, threshold=threshold,
        )
        if not gate_passed:
            git_checkout_clean_fn(repo_root)
            log_fn(f"[table-port:{table_name}] acceptance gate failed: {gate_reason}")
            remove_note = (
                f" Specifically: comment out (with a `// TODO({', '.join(must_remove)})` style "
                "marker per tag) every present-but-wrong member listed above -- never ship a "
                "wrong value." if must_remove else ""
            )
            outcome = critique_and_continue("gap_not_closed", gate_reason + remove_note, round_index)
            if outcome:
                return outcome
            continue

        approved, review_reason = review_fn(
            pseudo_gap, diff, review_config, call_model_fn=review_call_model_fn or call_model_fn,
            pick_model_fn=pick_model_fn,
        )
        if not approved:
            log_fn(f"[table-port:{table_name}] review REJECTED: {review_reason}")
            git_checkout_clean_fn(repo_root)
            rounds.append({"diff": diff, "reason": f"rejected by review: {review_reason}", "critique": review_reason})
            if round_index == max_repair_rounds - 1:
                return {
                    "table": table_name, "module": module, "status": "failed",
                    "reason": f"rejected by review: {review_reason}", "diff": diff, "rounds": rounds,
                }
            messages.append({
                "role": "user",
                "content": f"A reviewer rejected this fix: {review_reason}\nPlease resend a corrected diff.",
            })
            continue

        tests_passed, test_output = cargo_test_workspace_fn(repo_root)
        if not tests_passed:
            git_checkout_clean_fn(repo_root)
            reason = f"cargo test --workspace regressed:\n{_extract_test_failure_context(test_output)}"
            log_fn(f"[table-port:{table_name}] cargo test --workspace regressed, reverting")
            outcome = critique_and_continue("test_regressed", reason, round_index)
            if outcome:
                return outcome
            continue

        trailers = {
            "Format": format_name, "Table": table_name, "Worker": worker_label,
            "Verified": gate_reason,
        }
        git_commit_fn(
            f"feat({(module or table_name).lower()}): table-port {table_name}",
            repo_root, trailers=trailers,
        )
        log_fn(f"[table-port:{table_name}] LANDED ({gate_reason})")
        return {
            "table": table_name, "module": module, "status": "fixed",
            "gate_reason": gate_reason, "rounds": rounds,
        }

    return {
        "table": table_name, "module": module, "status": "failed",
        "reason": "exhausted repair rounds", "diff": diff, "rounds": rounds,
    }




def run_tag_loop(config, find_gaps_fn, fix_gap_fn, state_path,
                  git_checkout_clean_fn=None, repo_root=None, log_fn=print,
                  load_state_fn=load_tag_state, save_state_fn=save_tag_state,
                  max_rounds=None, max_fails=DEFAULT_MAX_TAG_FAILS, blacklist_full=False,
                  worker_id=None, claim_stale_seconds=DEFAULT_CLAIM_STALE_SECONDS,
                  max_distinct_tags=None,
                  refresh_worktree_fn=None, max_cluster_tags=1, landed_tags_path=None,
                  heartbeat_seconds=DEFAULT_HEARTBEAT_SECONDS, time_fn=time.time,
                  attribution=None, reachability_fn=None):
    """Loop-until-everything-found driver, blacklisting individual TAGS
    (never a whole format) after max_fails failed attempts each. State
    persists to disk at state_path, so the blacklist -- and each tag's
    attempt history -- survives across separate process runs, not just
    within this one call.

    Each failed attempt's diff/reason is appended to that tag's persisted
    "attempts" list (see fix_gap_fn's contract below) and handed back as
    previous_attempts on the next round targeting the same tag, so round N
    carries forward N-1 rounds of "here's what was already tried and why
    it failed" instead of starting from zero each time.

    Every read-modify-write of the shared state file happens inside a
    _state_locked closure (flock on state_path's sibling .lock file):
    load fresh under the lock, mutate, save atomically, release. Two
    workers sharing one state_path can therefore never interleave their
    load/save pairs and silently drop each other's claims or results.

    Once every currently-known tag is either fixed or blacklisted:
      - by default (blacklist_full=False), the reset deletes exactly the
        state keys THIS worker considered at claim-filter time this
        round (never the whole dict -- a shared state file also carries
        other formats'/workers' entries, which a bare `state = {}` used
        to wipe), and a fresh cycle starts, so a tag given up on under
        one random model pick gets a clean second chance later rather
        than being abandoned forever
      - with blacklist_full=True, the loop stops instead -- for a parallel
        run where the point IS to exhaust every tag once and report,
        rather than cycle forever

    worker_id, if given, tags this run's claim on a tag with an identity
    (see state's "claimed_by") and a timestamp, so multiple concurrent
    processes sharing the same state_path (a parallel run) don't both pick
    the same currently-unclaimed tag -- see claim_stale_seconds: a claim
    older than this is treated as abandoned (its owning process likely
    crashed) and can be re-claimed by anyone.

    heartbeat_seconds (0/None disables): while an attempt is in flight, a
    daemon thread re-stamps this worker's claimed_at on the leader and
    every cluster member at this cadence, through the same _state_locked
    path as everything else. Time-based rather than call-based on
    purpose: a call-based heartbeat starves exactly when claims most
    need to survive -- queued behind the rate governor or inside a long
    cargo build -- which is how sub-30-minute claim_stale_seconds
    produced double-claims in practice. The thread stops (threading.
    Event) the moment fix_gap_fn returns.

    time_fn is the clock used for claim stamps, staleness checks, and
    blacklisted_at -- injectable so tests can drive staleness and
    heartbeat behavior without real waiting.

    fix_gap_fn(tag_gap, config, previous_attempts) -> result dict; result
    must have "status" ("fixed" or anything else) and, when not fixed,
    "reason" and "diff" (the diff attempted, or None) for history tracking.

    max_rounds caps the number of attempts (None = run forever, until
    find_gaps_fn() reports zero gaps left anywhere, or blacklist_full's
    natural stop); tests pass a small cap instead of relying on that.

    max_distinct_tags, if given, caps how many different tags this one
    process will ever start work on (not total attempts -- a tag already
    started keeps getting retried across rounds same as always). Once
    that many distinct tags have been touched, the loop stops rather than
    picking up a brand-new one -- useful to bound one worker's share of a
    shared tag pool in a parallel run (see [parallel].max_tags_per_process
    in config.toml).

    max_cluster_tags, when > 1, lets the selected tag pull up to
    max_cluster_tags - 1 additional still-active sibling tags (same
    cluster_key: format, family, parser files) into one fix conversation
    as its "cluster_members". Members are claimed alongside the leader; a
    fixed/duplicate outcome clears every member's state entry, while a
    failure charges only the leader's fail budget and simply releases the
    members' claims. The default of 1 preserves the original
    one-tag-per-conversation behavior exactly.

    landed_tags_path, if given, is re-read at the top of every round
    (see load_landed_tags); any active tag whose tag_key is already in
    that set is skipped -- its state entry cleared like a duplicate --
    instead of re-deriving a fix the sweep has already merged.

    refresh_worktree_fn(), if given, is called at the start of every
    round before find_gaps_fn() -- see main()'s wiring to the real
    refresh_worktree(repo_root, base_ref), which fast-forwards this
    worktree onto the shared branch's latest commits. Since a tag can be
    retried across many rounds before it's fixed or blacklisted, this is
    what keeps that comparison from operating on an increasingly stale
    snapshot for however long that takes -- without it, another worker
    can fix and merge the exact same tag while this one is still
    grinding on it, and this one would never find out. None (the
    default) skips this entirely -- standalone/non-parallel runs have no
    shared branch to refresh against.

    attribution, if given, is the parsed gap-attribution.json document
    (spec S1/attribute_gaps.build_attribution's own output shape) used to
    resolve each candidate tag's canonical (module, table) -- spec S4
    item 5's cross-tier claim exclusion: a T1/T2 tag claim on a table
    with a live T3 TABLE-PORT claim (or on a module with a live T4
    FOUNDATION-UNLOCK claim) is excluded from `active` exactly like a
    same-tag claim by another worker already is, via claim_conflicts
    checked inside this SAME locked claim() closure -- no second lock
    acquisition. None (the default) skips resolution entirely
    (resolve_canonical_table already treats a missing attribution as
    "can't resolve, don't exclude"), so every existing caller keeps its
    exact prior behavior -- this is advisory and strictly additive.

    reachability_fn(tag_gap) -> (reachable, detail) is the PRE-DISPATCH
    gate (see make_reachability_fn/default_format_reachable). It runs
    after the claim and before fix_gap_fn, and ONLY a hard False skips:
    the tag is recorded in `skipped` with status "unreachable", its state
    entry is blacklisted with the reason attached, and not one model call
    is made. True and None (undetermined -- no binary yet, no sample on
    record) both proceed exactly as before.

    This is the cheapest possible answer to the class of target that
    caused 419 successful model calls to land zero tags on 2026-07-30: a
    format whose parser never executes cannot be fixed by any diff to
    that parser, and no amount of critique will tell a worker that. None
    (the default) disables the gate entirely, so every existing caller
    behaves exactly as it did before.
    """
    fixed, failed, skipped = [], [], []
    cycles_reset = 0
    round_num = 0
    seen_tag_keys = set()

    def locked(mutate_fn):
        """One serialized read-modify-write of the shared tag state --
        see _state_locked. Every state access in this loop goes through
        here; nothing reads or writes the file outside the flock."""
        return _state_locked(state_path, mutate_fn, load_state_fn, save_state_fn)

    def is_claimed_by_someone_else(entry):
        claimed_by = entry.get("claimed_by")
        if not claimed_by or claimed_by == worker_id:
            return False
        claimed_at = entry.get("claimed_at", 0)
        return (time_fn() - claimed_at) < claim_stale_seconds

    while max_rounds is None or round_num < max_rounds:
        round_num += 1
        if refresh_worktree_fn:
            refreshed, message = refresh_worktree_fn()
            if not refreshed:
                log_fn(f"worktree refresh skipped this round: {message}")
        gaps = find_gaps_fn()
        tag_gaps = expand_gaps_to_tags(gaps)

        if not tag_gaps:
            log_fn("All tags found -- nothing left to fix.")
            break

        landed = load_landed_tags(landed_tags_path) if landed_tags_path else set()
        # The explicit list of state keys this worker considered at
        # claim-filter time. If this round ends in blacklist exhaustion,
        # exactly these keys get deleted -- never a whole-dict reset
        # (which also wipes other formats'/workers' entries in a shared
        # state file) and never a key-prefix match (formats and squads
        # are not key prefixes).
        considered_keys = [tg["tag_key"] for tg in tag_gaps]

        def claim(state):
            """The whole claim step as one locked critical section:
            filter against fresh state (other workers' claims and
            blacklists), drop already-landed entries, then either claim
            a leader (plus cluster members) or decide how this round
            ends. The scoped exhaustion reset lives here too, because it
            must share the lock with the all-blacklisted check it
            depends on -- checked-then-reset across two lock
            acquisitions would race a concurrent claimant."""
            def resolve_for(tg):
                return resolve_canonical_table(tg["tag_key"], attribution)

            def conflicts_with_live_jobs(tg):
                # Spec S4 item 5: exclude a T1/T2 candidate whose
                # canonical table/module has a live T3/T4 job claim on
                # it (see claim_conflicts) -- resolved best-effort via
                # `attribution`; a candidate whose module can't be
                # resolved (attribution absent, or the "unknown" bucket)
                # is never excluded on this basis, per
                # resolve_canonical_table's own contract.
                module, table = resolve_for(tg)
                if module is None:
                    return False
                candidate = {
                    "tier": "T1", "tag_key": tg["tag_key"],
                    "canonical_table": table, "canonical_module": module,
                }
                live = gather_live_claims(state, time_fn, claim_stale_seconds, exclude_key=tg["tag_key"])
                return claim_conflicts(live, candidate)

            active = [
                tg for tg in tag_gaps
                if not state.get(tg["tag_key"], {}).get("blacklisted")
                and not is_claimed_by_someone_else(state.get(tg["tag_key"], {}))
                and (max_distinct_tags is None or len(seen_tag_keys) < max_distinct_tags
                     or tg["tag_key"] in seen_tag_keys)
                and not conflicts_with_live_jobs(tg)
            ]

            landed_hits = [tg["tag_key"] for tg in active if tg["tag_key"] in landed]
            for key in landed_hits:
                state.pop(key, None)
            active = [tg for tg in active if tg["tag_key"] not in landed]

            if not active and max_distinct_tags is not None and len(seen_tag_keys) >= max_distinct_tags:
                return state, ("stop_max_distinct", landed_hits, None, None)

            if not active:
                all_blacklisted = all(state.get(tg["tag_key"], {}).get("blacklisted") for tg in tag_gaps)
                if blacklist_full and all_blacklisted:
                    return state, ("stop_blacklist_full", landed_hits, None, None)
                if all_blacklisted:
                    for key in considered_keys:
                        clear_blacklist_keeping_history(state, key)
                    return state, ("reset", landed_hits, None, None)
                return state, ("wait", landed_hits, None, None)

            tag_gap = active[0]
            entry = state.setdefault(tag_gap["tag_key"], {"fails": 0, "blacklisted": False, "attempts": []})
            entry["claimed_by"] = worker_id
            entry["claimed_at"] = time_fn()

            members = []
            if max_cluster_tags > 1:
                leader_key = cluster_key(tag_gap)
                for cand in active[1:]:
                    if len(members) >= max_cluster_tags - 1:
                        break
                    if cluster_key(cand) == leader_key:
                        members.append(cand)
                for m in members:
                    m_entry = state.setdefault(m["tag_key"], {"fails": 0, "blacklisted": False, "attempts": []})
                    m_entry["claimed_by"] = worker_id
                    m_entry["claimed_at"] = time_fn()
                    m_entry["tier"] = "T2"
                    m_module, m_table = resolve_for(m)
                    if m_module is not None:
                        m_entry["canonical_module"] = m_module
                    if m_table is not None:
                        m_entry["canonical_table"] = m_table
            # Spec S4 item 5 / KPI (spec §5's calls-per-landed-tag): tier
            # is "T2" for a clustered sibling-family claim, "T1"
            # otherwise -- also lets a future T3/T4 claim (see
            # gather_live_claims/claim_conflicts) recognize this entry's
            # kind, and lets watch_parallel_fix.py's tier KPI attribute
            # a landed tag correctly.
            entry["tier"] = "T2" if members else "T1"
            leader_module, leader_table = resolve_for(tag_gap)
            if leader_module is not None:
                entry["canonical_module"] = leader_module
            if leader_table is not None:
                entry["canonical_table"] = leader_table
            if members:
                tag_gap = dict(tag_gap, cluster_members=members)
            return state, ("claimed", landed_hits, tag_gap, list(entry.get("attempts", [])))

        outcome, landed_hits, tag_gap, previous_attempts = locked(claim)
        for key in landed_hits:
            log_fn(f"[{key}] skipped -- already landed via sweep")

        if outcome == "stop_max_distinct":
            log_fn(f"Reached max_distinct_tags={max_distinct_tags} for this process -- stopping.")
            break
        if outcome == "stop_blacklist_full":
            log_fn(f"All {len(tag_gaps)} tag(s) are blacklisted -- stopping (--blacklist-full).")
            break
        if outcome == "reset":
            log_fn(
                f"All {len(tag_gaps)} remaining tag(s) are blacklisted -- "
                f"resetting this worker's {len(considered_keys)} tag entr"
                f"{'y' if len(considered_keys) == 1 else 'ies'} and starting a new cycle"
            )
            cycles_reset += 1
            continue
        if outcome == "wait":
            # Nothing blacklisted, but everything currently claimed by
            # other (non-stale) workers -- wait rather than busy-loop.
            log_fn("All remaining tags are claimed by other workers -- waiting")
            time.sleep(5)
            continue

        seen_tag_keys.add(tag_gap["tag_key"])
        for m in tag_gap.get("cluster_members") or []:
            seen_tag_keys.add(m["tag_key"])
        if tag_gap.get("cluster_members"):
            log_fn(f"clustered {len(tag_gap['cluster_members'])} sibling tag(s) with {tag_gap['tag_key']}")

        # Pre-dispatch reachability gate. Deliberately here rather than in
        # claim()'s filter: this shells out to the oxidex binary, and a
        # subprocess has no business running inside the state flock.
        # Deliberately a hard `is False` rather than a falsy check: an
        # undetermined verdict (None) must dispatch, never skip.
        if reachability_fn is not None:
            reachable, detail = reachability_fn(tag_gap)
            if reachable is False:
                reason = format_unreachable_reason(tag_gap["format"], detail)
                log_fn(f"[{tag_gap['tag_key']}] SKIPPED (unreachable): {reason}")
                skipped.append({
                    "tag_key": tag_gap["tag_key"], "format": tag_gap["format"],
                    "status": "unreachable", "reason": reason,
                })

                def release_unreachable(state, _reason=reason):
                    """Blacklist the leader (so this process stops
                    re-picking a target no diff can close) and release
                    every member's claim. Charged to no fail budget: the
                    tag is not at fault, its format's parser is."""
                    for key in ([tag_gap["tag_key"]]
                                + [m["tag_key"] for m in tag_gap.get("cluster_members") or []]):
                        member_entry = state.setdefault(
                            key, {"fails": 0, "blacklisted": False, "attempts": []},
                        )
                        member_entry.pop("claimed_by", None)
                        member_entry.pop("claimed_at", None)
                        member_entry["blacklisted"] = True
                        member_entry["blacklisted_at"] = time_fn()
                        member_entry["blacklisted_by"] = worker_id
                        member_entry["blacklist_reason"] = _reason
                        member_entry["unreachable"] = True
                    return state, None

                locked(release_unreachable)
                continue

        # One line per round naming both the round number and the tag --
        # the single source watch_parallel_fix.py's dashboard reads to
        # show "what iteration is this worker on, and on what tag" without
        # having to infer it from whatever bracketed status line happens
        # to be logged deeper inside fix_gap.
        log_fn(f"round {round_num}: attempting {tag_gap['tag_key']}")

        claimed_keys = [tag_gap["tag_key"]] + [m["tag_key"] for m in tag_gap.get("cluster_members") or []]

        def heartbeat_touch(state):
            now = time_fn()
            for key in claimed_keys:
                entry = state.get(key)
                if entry and entry.get("claimed_by") == worker_id:
                    entry["claimed_at"] = now
            return state, None

        stop_heartbeat = threading.Event()

        def heartbeat_loop():
            # Event.wait doubles as the sleep: it returns True the
            # moment the attempt ends and the event is set, so the
            # thread never outlives the attempt by more than one
            # (milliseconds-long) state touch.
            while not stop_heartbeat.wait(heartbeat_seconds):
                try:
                    locked(heartbeat_touch)
                except Exception as e:
                    # One failed touch must NOT kill this thread: an
                    # unhandled raise here dies via threading's default
                    # excepthook (stderr only) while the attempt keeps
                    # running for hours -- silently reverting to exactly
                    # the stale-claim/double-claim behavior the heartbeat
                    # exists to prevent. The triggers are transient by
                    # nature (a torn read of state written by a pre-flock
                    # worker during the mixed-version rollout window,
                    # ENOSPC/EACCES on save), so log through log_fn --
                    # where a human is actually looking -- and beat again
                    # next cadence.
                    log_fn(f"claim heartbeat touch failed ({e!r}) -- retrying next beat")

        # Time-based heartbeat -- see the docstring. Daemon so a worker
        # dying mid-attempt can never be kept alive by its own
        # heartbeat; its claim then goes stale and is re-claimable,
        # which is exactly the semantics claim_stale_seconds promises.
        heartbeat_thread = None
        if heartbeat_seconds:
            heartbeat_thread = threading.Thread(
                target=heartbeat_loop, name=f"claim-heartbeat-{worker_id or 'solo'}", daemon=True,
            )
            heartbeat_thread.start()

        try:
            result = fix_gap_fn(tag_gap, config, previous_attempts)
        finally:
            stop_heartbeat.set()
            if heartbeat_thread is not None:
                heartbeat_thread.join()

        def record(state):
            """Locked result step: re-fetch this tag's entry from fresh
            state (other workers may have touched other tags meanwhile),
            release the claims, and apply the outcome. Returns whether
            this outcome just blacklisted the tag, so the caller can run
            git cleanup OUTSIDE the lock -- a git subprocess has no
            business extending the flock critical section."""
            entry = state.setdefault(tag_gap["tag_key"], {"fails": 0, "blacklisted": False, "attempts": []})
            entry.pop("claimed_by", None)
            entry.pop("claimed_at", None)
            blacklisted_now = False

            if result["status"] == "fixed":
                fixed.append({"tag_key": tag_gap["tag_key"], **result})
                state.pop(tag_gap["tag_key"], None)
                for m in tag_gap.get("cluster_members") or []:
                    state.pop(m["tag_key"], None)
                log_fn(f"[{tag_gap['tag_key']}] FIXED")
            elif result["status"] == "duplicate":
                # Already fixed elsewhere (see fix_gap's detect_duplicate_fn)
                # -- this worker's own worktree was stale when it started,
                # not a real failure of this tag, so don't count it against
                # the fail budget or let it march toward blacklisting; just
                # drop any stale attempt history the same way a genuine fix
                # would, and move on to a different tag next round.
                skipped.append({"tag_key": tag_gap["tag_key"], **result})
                state.pop(tag_gap["tag_key"], None)
                for m in tag_gap.get("cluster_members") or []:
                    state.pop(m["tag_key"], None)
                log_fn(f"[{tag_gap['tag_key']}] SKIPPED (already fixed elsewhere)")
            else:
                failed.append({"tag_key": tag_gap["tag_key"], **result})
                # A cluster failure charges only the leader -- members are
                # simply released (claims dropped) so they stay eligible,
                # individually or under a future leader, without inheriting
                # a fail count for an attempt that was the leader's.
                for m in tag_gap.get("cluster_members") or []:
                    me = state.get(m["tag_key"])
                    if me:
                        me.pop("claimed_by", None)
                        me.pop("claimed_at", None)
                rounds_list = result.get("rounds") or [
                    {"diff": result.get("diff"), "reason": result.get("reason", "unknown"), "critique": None}
                ]
                infra_only = all(
                    str(r.get("reason", "")).startswith(INFRA_FAILURE_PREFIX) for r in rounds_list
                )
                if infra_only:
                    # Every round was an infrastructure failure (rate limit,
                    # network, provider error -- see INFRA_FAILURE_PREFIX)
                    # -- that isn't the tag's fault, and counting it lets a
                    # rate-limit storm blacklist every active tag; just as
                    # bad, persisting the junk rounds clutters every future
                    # prompt for this tag. Report it (failed above) but
                    # charge nothing: no fail increment, no attempt history,
                    # no blacklist check.
                    log_fn(
                        f"[{tag_gap['tag_key']}] infrastructure failure "
                        f"(not counted against fail budget): {result.get('reason', 'unknown')}"
                    )
                else:
                    entry["fails"] = entry.get("fails", 0) + 1
                    # fix_gap's own "rounds" is every internal repair sub-attempt
                    # (build failure, gap-not-closed, test regression, or review
                    # rejection), each with its own critique -- persist all of
                    # them, not just the call's final outcome, so a future round
                    # sees the whole arc of what was tried and why each step
                    # failed. Falls back to one flattened entry (no critique) for
                    # a caller whose fix_gap_fn doesn't return "rounds".
                    # Infrastructure-failure rounds inside a mixed result are
                    # dropped -- they carry no signal about the tag -- keeping
                    # only the real-signal rounds (with a fall-back to
                    # everything should filtering somehow leave nothing).
                    real_rounds = [
                        r for r in rounds_list
                        if not str(r.get("reason", "")).startswith(INFRA_FAILURE_PREFIX)
                    ]
                    for sub_round in real_rounds or rounds_list:
                        entry.setdefault("attempts", []).append({
                            "round": entry["fails"], "diff": sub_round.get("diff"),
                            "reason": sub_round.get("reason", "unknown"), "critique": sub_round.get("critique"),
                        })
                    if entry["fails"] >= max_fails:
                        entry["blacklisted"] = True
                        # Both persisted alongside "blacklisted" (not just logged)
                        # so a dashboard reading tag-state.json later -- possibly
                        # long after this worker's own log has been truncated by a
                        # respawn -- can still answer "when" and "by which worker"
                        # for every blacklist event, not just the current count.
                        entry["blacklisted_at"] = time_fn()
                        entry["blacklisted_by"] = worker_id
                        blacklisted_now = True
                        log_fn(f"[{tag_gap['tag_key']}] blacklisted after {entry['fails']} failed attempts")
                    else:
                        log_fn(f"[{tag_gap['tag_key']}] failed attempt {entry['fails']}/{max_fails}")
                state[tag_gap["tag_key"]] = entry

            return state, blacklisted_now

        if locked(record) and git_checkout_clean_fn and repo_root:
            git_checkout_clean_fn(repo_root)

    return {
        "rounds": round_num,
        "fixed": fixed,
        "failed": failed,
        "skipped": skipped,
        "cycles_reset": cycles_reset,
        "distinct_tags_seen": len(seen_tag_keys),
    }


DEFAULT_TAG_STATE_PATH = OXIDEX_HOME / "logs" / "model-fix-tag-state.json"
DEFAULT_CONFIG_PATH = REPO_ROOT / "config.toml"


def load_toml_config(path):
    """Load config.toml. Returns the parsed table dict, or None if the file
    doesn't exist (a missing file is a caller-level error, not silently
    defaulted -- there's no sensible default for a list of models/API
    keys)."""
    if not path.is_file():
        return None
    with open(path, "rb") as f:
        return tomllib.load(f)


_KNOWN_MODEL_SPEC_KEYS = {"name", "base_url", "api_key", "phase", "reasoning_effort"}
# "table" (spec S3): a model tagged phase="table" is reserved for T3
# TABLE-PORT / T4 FOUNDATION-UNLOCK jobs -- "the strongest configured
# model" per the spec, kept out of the ordinary per-tag explore/patch
# rotation so a table/foundation job (which the spec budgets far more
# tokens/repair-rounds for) always gets the pool's best model rather than
# whichever one pick_model_fn happens to land on.
_VALID_MODEL_PHASES = {"explore", "patch", "table"}


def models_for_phase(models, phase):
    """Filter a model pool to entries tagged for `phase` -- untagged
    entries (phase absent/None) are eligible for every phase. Falls back
    to the full pool when the filter would be empty, so a config with no
    phase tags behaves exactly as before this feature existed.

    phase="table" (spec S3) selects the model(s) reserved for T3/T4 job
    prompts -- see attempt_table_port/attempt_foundation_job, which build
    their own job-scoped config via
    dict(config, models=models_for_phase(config["models"], "table"))
    before delegating to the ordinary attempt_build_fn (whose ITS OWN
    internal explore/patch phase selection then operates over that
    already-narrowed pool)."""
    matching = [m for m in models if m.get("phase") in (None, phase)]
    return matching or models


def _normalize_model_spec(entry, default_base_url, default_api_key):
    """Turn one models[] entry into a {"name", "base_url", "api_key"} dict.

    A plain string entry (e.g. "glm5.2-fast") uses the table's own
    base_url/api_key. A table entry (TOML inline table or [[worker.models]]
    array-of-tables) may override base_url/api_key individually, so a
    single pool can mix providers -- e.g. one wafer.ai model alongside a
    Fireworks-hosted one with its own key.

    Only name/base_url/api_key/phase/reasoning_effort are recognized on an
    entry -- max_tokens, stream, thinking, and temperature belong on the
    parent [worker]/[reviewer] table, shared across every model in the
    pool. A misplaced key there raises immediately instead of being
    silently dropped, which is exactly what happened when max_tokens got
    written under [[worker.models]] instead of [worker]: the value never
    took effect, and nothing in the run reported that.
    """
    if isinstance(entry, str):
        return {"name": entry, "base_url": default_base_url, "api_key": default_api_key,
                "phase": None, "reasoning_effort": None}
    unknown = set(entry) - _KNOWN_MODEL_SPEC_KEYS
    if unknown:
        raise ValueError(
            f"unrecognized key(s) {sorted(unknown)} on a models[] entry ({entry.get('name', '?')!r}) -- "
            "only name/base_url/api_key/phase/reasoning_effort belong on an individual model entry; "
            "max_tokens, stream, thinking, and temperature belong on the parent "
            "[worker]/[reviewer] table instead, shared across every model in the pool"
        )
    phase = entry.get("phase")
    if phase is not None and phase not in _VALID_MODEL_PHASES:
        raise ValueError(
            f"invalid phase {phase!r} on models[] entry {entry.get('name', '?')!r} -- "
            f"must be one of {sorted(_VALID_MODEL_PHASES)} (or omitted for both phases)"
        )
    return {
        "name": entry["name"],
        "base_url": entry.get("base_url", default_base_url),
        "api_key": entry.get("api_key", default_api_key),
        "phase": phase,
        "reasoning_effort": entry.get("reasoning_effort"),
    }


def _normalize_model_config(table):
    """Turn a [worker]/[reviewer] TOML table into this module's config dict
    shape, filling in the same defaults main() used to apply to env vars."""
    default_base_url = table.get("base_url")
    default_api_key = table.get("api_key")
    return {
        "base_url": default_base_url,
        "api_key": default_api_key,
        "models": [
            _normalize_model_spec(m, default_base_url, default_api_key)
            for m in (table.get("models") or [])
        ],
        "max_tokens": table.get("max_tokens", DEFAULT_MAX_TOKENS),
        "max_prompt_tokens": table.get("max_prompt_tokens", DEFAULT_MAX_PROMPT_TOKENS),
        "reasoning_effort": table.get("reasoning_effort", "max"),
        "max_prompt_tags": table.get("max_prompt_tags", DEFAULT_MAX_PROMPT_TAGS),
        "max_prompt_file_bytes": table.get("max_prompt_file_bytes", DEFAULT_MAX_PROMPT_FILE_BYTES),
        "stream": table.get("stream", True),
        "prompt_cache": table.get("prompt_cache", "auto"),
        "thinking": table.get("thinking", True),
        "temperature": table.get("temperature", 0),
        "timeout": table.get("timeout", 120),
        "deadline_seconds": table.get("deadline_seconds", DEFAULT_DEADLINE_SECONDS),
        "max_request_turns": table.get("max_request_turns", DEFAULT_MAX_REQUEST_TURNS),
        "max_request_turns_ceiling": table.get(
            "max_request_turns_ceiling", DEFAULT_MAX_REQUEST_TURNS_CEILING),
        "max_repair_rounds": table.get("max_repair_rounds", DEFAULT_MAX_REPAIR_ROUNDS),
        "max_review_rounds": table.get("max_review_rounds", DEFAULT_MAX_REVIEW_ROUNDS),
        "max_request_repeats": table.get("max_request_repeats", DEFAULT_MAX_REQUEST_REPEATS),
        "max_verify_turns": table.get("max_verify_turns", DEFAULT_MAX_VERIFY_TURNS),
        "compaction_trigger_tokens": table.get("compaction_trigger_tokens", DEFAULT_COMPACTION_TRIGGER_TOKENS),
        "compaction_keep_recent_turns": table.get("compaction_keep_recent_turns", DEFAULT_COMPACTION_KEEP_RECENT_TURNS),
        "compaction_min_elide_tokens": table.get("compaction_min_elide_tokens", DEFAULT_COMPACTION_MIN_ELIDE_TOKENS),
        "max_retries": table.get("max_retries", DEFAULT_MAX_RETRIES),
        "retry_backoff_seconds": table.get("retry_backoff_seconds", DEFAULT_RETRY_BACKOFF_SECONDS),
        "max_retry_backoff_seconds": table.get("max_retry_backoff_seconds", DEFAULT_MAX_RETRY_BACKOFF_SECONDS),
        "governor_calls_per_minute": table.get("governor_calls_per_minute", DEFAULT_GOVERNOR_CALLS_PER_MINUTE),
        "governor_burst": table.get("governor_burst", DEFAULT_GOVERNOR_BURST),
        "max_cluster_tags": table.get("max_cluster_tags", DEFAULT_MAX_CLUSTER_TAGS),
        "use_sccache": table.get("use_sccache", True),
        "claim_stale_seconds": table.get("claim_stale_seconds", DEFAULT_CLAIM_STALE_SECONDS),
        "heartbeat_seconds": table.get("heartbeat_seconds", DEFAULT_HEARTBEAT_SECONDS),
        # Section 6 / K5 knobs (Phase 1 spec).
        "reviewer_max_prompt_tokens": table.get(
            "reviewer_max_prompt_tokens", DEFAULT_REVIEWER_MAX_PROMPT_TOKENS),
        "learning_budget_tokens": table.get("learning_budget_tokens", DEFAULT_LEARNING_BUDGET_TOKENS),
        "parser_floor_tokens": table.get("parser_floor_tokens", DEFAULT_PARSER_FLOOR_TOKENS),
        "lessons_tail_kb": table.get("lessons_tail_kb", DEFAULT_LESSONS_TAIL_KB),
        # Spec section 5: cross-process cargo build/test slot ceiling (see
        # find_tag_gaps.build_semaphore) -- max concurrent cargo
        # build/check/test invocations across every worker+merger sharing
        # this host, so a full round of workers all rechecking gaps at
        # once can't oversubscribe the host's cores by linking
        # concurrently.
        "build_semaphore": table.get("build_semaphore", DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS),
    }


def normalize_table_job_config(toml_data):
    """Spec S3's `[table_job]` table (T3 TABLE-PORT / T4 FOUNDATION-UNLOCK
    knobs) -- a sibling of [worker]/[reviewer], not nested inside either,
    since a table/foundation job's prompt budget and repair-round ceiling
    are independent of the per-tag fixer's own knobs. Missing section (the
    common case today -- these tiers are opt-in) falls back to the spec's
    own defaults entirely, so a config.toml with no [table_job] table at
    all behaves exactly as if one were present with every default value."""
    section = toml_data.get("table_job") or {}
    return {
        "max_prompt_tokens": section.get("max_prompt_tokens", DEFAULT_TABLE_JOB_MAX_PROMPT_TOKENS),
        "max_repair_rounds": section.get("max_repair_rounds", DEFAULT_TABLE_JOB_MAX_REPAIR_ROUNDS),
        # "model" per spec S3 -- strongest configured model -- is resolved
        # via models_for_phase(pool, "table") at call time (see
        # attempt_table_port/attempt_foundation_job), not stored here;
        # this table only carries the two numeric knobs.
    }


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--config", default=str(DEFAULT_CONFIG_PATH),
        help="Path to config.toml (see config.example.toml)",
    )
    parser.add_argument(
        "--models",
        help="Comma-separated override for the worker's model pool "
             "(replaces config.toml's [worker].models entirely)",
    )
    parser.add_argument(
        "--review-models",
        help="Comma-separated override for the reviewer's model pool "
             "(replaces config.toml's [reviewer].models entirely)",
    )
    # A fixed /tmp default is a race-condition concern on shared multi-user
    # systems; this is a single-developer local CLI tool, and the value is
    # always overridable via EXIFTOOL_CACHE_DIR/--cache-dir.
    parser.add_argument(
        "--cache-dir",
        default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"),  # nosec B108
    )
    parser.add_argument(
        "--only-format",
        default=os.environ.get("MODEL_FIX_ONLY_FORMAT"),
        help="Scope the loop to a single format (e.g. JPEG, NEF). Uses the "
             "fast single-format comparison instead of the full corpus scan; "
             "requires the combined-samples cache to already exist from a "
             "prior full run (see find_tag_gaps.py's own --only-format).",
    )
    parser.add_argument(
        "--max-tag-fails", type=int, default=DEFAULT_MAX_TAG_FAILS,
        help=f"Failed attempts on one tag before it's blacklisted (default: {DEFAULT_MAX_TAG_FAILS}). "
             "Each failed attempt's diff/reason is carried forward as guidance to the next.",
    )
    parser.add_argument(
        "--blacklist-full", action="store_true",
        help="Stop once every known tag is blacklisted or fixed, instead of the default "
             "behavior of clearing the blacklist and starting a fresh cycle forever. "
             "Intended for a parallel run where the point is to exhaust the tag pool once.",
    )
    parser.add_argument(
        "--worker-id", default=os.environ.get("MODEL_FIX_WORKER_ID"),
        help="Identity used to claim a tag in --tag-state-path so concurrent processes sharing "
             "the same state file don't both attempt the same tag; also used to name this "
             "process's prompt log (process-<id>-prompt.log).",
    )
    parser.add_argument(
        "--base-ref", default=DEFAULT_BASE_REF,
        help="Shared branch this worktree is refreshed against. run_tag_loop fast-forwards "
             "this worktree onto its latest commits at the start of every round, so a tag "
             "retried across many rounds doesn't keep comparing against an increasingly stale "
             "snapshot while other workers merge in fixes elsewhere. Defaults to "
             f"{DEFAULT_BASE_REF}; pass --base-ref '' to disable for a standalone run with no "
             "shared branch. Defaulting rather than leaving this None is deliberate -- a live "
             "fleet ran for days with it unset, so refresh_worktree_fn was None and every "
             "worker measured against the commit it started on. 606 of 928 tracked tags (65%%) "
             "went stale that way: already fixed and merged upstream, still being re-attempted.",
    )
    parser.add_argument(
        "--tag-state-path", default=str(DEFAULT_TAG_STATE_PATH),
        help=f"Where the per-tag blacklist/fail-count/attempt-history state persists "
             f"(default: {DEFAULT_TAG_STATE_PATH}). Point multiple concurrent processes at "
             "the same path to coordinate them via --worker-id claims.",
    )
    parser.add_argument(
        "--prompt-log-dir", default=str(OXIDEX_HOME / "logs" / "tag-fix-prompts"),
        help="Directory for process-<worker-id>-prompt.log, which every round's full prompt "
             "is appended to (also printed to stdout).",
    )
    parser.add_argument(
        "--max-tags-per-process", type=int, default=None,
        help="Cap how many distinct tags this one process will start work on before stopping "
             "(a tag already started keeps getting retried as normal). Default: "
             f"[parallel].max_tags_per_process in config.toml, or {DEFAULT_MAX_TAGS_PER_PROCESS} if absent.",
    )
    parser.add_argument(
        "--tags-found-log", default=str(OXIDEX_HOME / "logs" / "tags-found.log"),
        help="Every tag actually fixed gets one appended line here (timestamp, worker id, tag "
             "key, gaps closed) -- point every worker at the same path (outside any worker's own "
             "worktree, which gets reset between rounds) for a single shared record of exactly "
             f"which tags were found across a parallel run. Default: {OXIDEX_HOME / 'logs' / 'tags-found.log'}",
    )
    parser.add_argument(
        "--sweep-review-log", default=str(OXIDEX_HOME / "logs" / "sweep-review-history.jsonl"),
        help="scripts/log_sweep_review.py's JSONL log of human sweep-review verdicts "
             "(accepted/rejected, with reasons) -- read back into the prompt for this gap's "
             f"format if present. Default: {OXIDEX_HOME / 'logs' / 'sweep-review-history.jsonl'}",
    )
    parser.add_argument(
        "--gap-attribution", default=str(OXIDEX_HOME / "logs" / "gap-attribution.json"),
        help="scripts/attribute_gaps.py's gap-attribution.json (spec S1) -- read once at "
             "startup (best-effort; missing/corrupt is treated as 'unavailable', same as every "
             "other optional advisory input here) and used ONLY for spec S4 item 5's cross-tier "
             "claim exclusion (resolve_canonical_table) -- a T1/T2 tag claim is refused when its "
             "table has a live T3 TABLE-PORT claim, and vice versa. Purely advisory: omitting "
             f"or losing this file never blocks an ordinary claim. Default: "
             f"{OXIDEX_HOME / 'logs' / 'gap-attribution.json'}",
    )
    args = parser.parse_args(argv)

    config_path = Path(args.config)
    toml_data = load_toml_config(config_path)
    if toml_data is None:
        print(f"{config_path} not found -- see config.example.toml", file=sys.stderr)
        return 1

    worker_table = toml_data.get("worker")
    if not worker_table:
        print(f"{config_path} is missing a [worker] table", file=sys.stderr)
        return 1

    try:
        config = _normalize_model_config(worker_table)
        if args.models:
            config["models"] = [
                _normalize_model_spec(m.strip(), config["base_url"], config["api_key"])
                for m in args.models.split(",") if m.strip()
            ]

        review_config = _normalize_model_config(toml_data.get("reviewer") or worker_table)
        if args.review_models:
            review_config["models"] = [
                _normalize_model_spec(m.strip(), review_config["base_url"], review_config["api_key"])
                for m in args.review_models.split(",") if m.strip()
            ]
    except ValueError as e:
        print(f"{config_path}: {e}", file=sys.stderr)
        return 1

    for label, cfg in (("worker", config), ("reviewer", review_config)):
        if not cfg["models"] or not all(m["base_url"] and m["api_key"] for m in cfg["models"]):
            print(
                f"{config_path}'s [{label}] table needs a non-empty models list, "
                "and every entry needs a base_url and api_key (either its own or "
                "the table's default) (or --models/--review-models)",
                file=sys.stderr,
            )
            return 1

    # Before the first cargo subprocess: let config.toml's use_sccache knob
    # (worker table, default on) reach cargo_env's OXIDEX_USE_SCCACHE gate.
    os.environ["OXIDEX_USE_SCCACHE"] = "1" if config.get("use_sccache", True) else "0"

    # Spec section 5 build semaphore: every real cargo build/check/test
    # call site in this run shares the one cross-process slot ceiling
    # (config["build_semaphore"], default DEFAULT_BUILD_SEMAPHORE_MAX_HOLDERS)
    # -- partials so fix_gap's own default (semaphore disabled) is only
    # ever hit by callers/tests that don't thread these through.
    cargo_build_fn = functools.partial(
        cargo_build, semaphore_path=DEFAULT_BUILD_SEMAPHORE_PATH,
        semaphore_max_holders=config["build_semaphore"],
    )
    cargo_check_fn = functools.partial(
        cargo_check, semaphore_path=DEFAULT_BUILD_SEMAPHORE_PATH,
        semaphore_max_holders=config["build_semaphore"],
    )
    cargo_test_workspace_fn = functools.partial(
        cargo_test_workspace, semaphore_path=DEFAULT_BUILD_SEMAPHORE_PATH,
        semaphore_max_holders=config["build_semaphore"],
    )
    cargo_test_targeted_fn = functools.partial(
        cargo_test_targeted, semaphore_path=DEFAULT_BUILD_SEMAPHORE_PATH,
        semaphore_max_holders=config["build_semaphore"],
    )

    # This process's identity in every shared artifact: manifest.log lines,
    # request/response/diff filenames, the per-worker prompt log, and the
    # /tmp/tagcmp-* comparison outputs (out_suffix below). Standalone runs
    # get "1" so nothing is ever unlabeled.
    worker_label = args.worker_id or "1"

    def find_gaps_fn():
        if args.only_format:
            # out_suffix isolates this worker's comparison output from
            # every other process re-checking the same format -- see
            # run_format_comparison. The shared fixed /tmp path used to
            # let two same-format workers overwrite each other's report
            # mid-recheck and corrupt tag_still_open verdicts.
            report_path = run_format_comparison(
                args.only_format, args.cache_dir, out_suffix=worker_label,
                semaphore_path=DEFAULT_BUILD_SEMAPHORE_PATH,
                semaphore_max_holders=config["build_semaphore"],
            )
        else:
            report_path = run_full_comparison(args.cache_dir)
        gaps = group_gaps_by_format(load_comparison_report(report_path))
        if args.only_format:
            gaps = [g for g in gaps if g["format"] == args.only_format]
        return gaps

    # Audit trail of every diff the model produces, applied or not -- so
    # "did it actually change code, and when" never has to be inferred from
    # a one-line summary again. attempt_build's own git_checkout_clean_fn
    # calls still revert a rejected/failed diff from the working tree right
    # after this logs it, so this directory is the only durable record of
    # what was tried each round.
    diff_log_dir = OXIDEX_HOME / "logs" / "model-fix-diffs"
    diff_log_dir.mkdir(parents=True, exist_ok=True)
    manifest_path = diff_log_dir / "manifest.log"

    def logging_git_apply(diff_text, repo_root):
        # worker_label in the filename: diff_log_dir is one shared
        # OXIDEX_HOME location, and two workers applying diffs in the
        # same second used to overwrite each other's .diff artifact --
        # the manifest then pointed at a file holding the OTHER worker's
        # diff.
        ts = time.strftime("%Y-%m-%dT%H:%M:%S")
        # git_apply_with_rung, not git_apply: rung is the whole point of the
        # tolerance ladder being measurable. "rung=exact" vs "rung=context1"
        # in the manifest is what tells us, over the next few thousand
        # diffs, whether the looser rungs are earning their keep or quietly
        # rescuing diffs that should have been rejected.
        applied, msg, rung = git_apply_with_rung(diff_text, repo_root)
        diff_path = diff_log_dir / f"{ts}-{worker_label}-{'applied' if applied else 'rejected'}.diff"
        diff_path.write_text(diff_text)
        with manifest_path.open("a") as f:
            f.write(
                f"{ts} worker={worker_label} applied={applied} rung={rung} "
                f"file={diff_path.name} apply_msg={msg[:200]!r}\n"
            )
        return applied, msg

    def timestamped_log(msg):
        print(f"[{time.strftime('%Y-%m-%dT%H:%M:%S')}] {msg}")

    # Audit trail of every actual API call (fixer and reviewer both funnel
    # through this -- see make_logging_call_model's two phase-tagged
    # instances below) -- request params + prompt saved before the call,
    # response (or the exact error) saved right after, so "is it even
    # talking to the model, and what did it get back" never has to be
    # guessed at from a timeout/exception message alone.
    req_log_dir = OXIDEX_HOME / "logs" / "model-fix-requests"
    req_log_dir.mkdir(parents=True, exist_ok=True)
    req_manifest_path = req_log_dir / "manifest.log"
    cache_stats_path = req_log_dir / "cache-stats.log"
    # Structured, machine-readable superset of the manifest line: one JSON
    # object per ATTEMPT (not per call), carrying the error class a
    # dashboard needs to answer "how many of those 429s were rpm and how
    # many were a spent budget?" without parsing prose. manifest.log stays
    # exactly as it is -- watch_parallel_fix.py and the existing tooling
    # parse it, and this must not disturb them.
    call_events_path = OXIDEX_HOME / "logs" / "model-calls.jsonl"

    def append_call_event(event):
        event["worker"] = worker_label
        with call_events_path.open("a") as f:
            f.write(json.dumps(event) + "\n")

    def make_logging_call_model(phase, tier="T1"):
        """Build a call_model_fn wrapper tagged with phase ("fixer" or
        "reviewer") in every manifest.log line it writes. fix_gap used to
        thread one shared closure into both attempt_build and review_fn,
        which made every manifest.log entry ambiguous about which side
        made the call -- fine for a human skimming the log, but useless
        for a dashboard trying to report separate fixer/reviewer request
        counts and latencies without guessing. Two instances of this
        (one per phase) replace that single shared closure.

        Every line also carries worker=worker_label: req_log_dir is a
        single OXIDEX_HOME-fixed location shared by every worker/format
        process (not a per-worktree path), so without this tag there'd be
        no way to tell whose call a given manifest.log line was after the
        fact -- see watch_parallel_fix.py's entries_for_worker.

        tier (spec section 5's KPI: "calls-per-landed-tag per tier"),
        default "T1" -- the ordinary per-tag path this main() wires
        below always uses the default; run_foundation_job_once/
        run_table_job_once (T4/T3) build their own instances of this
        closure with tier="T4"/"T3" so their manifest.log lines are
        distinguishable from ordinary per-tag calls without disturbing
        every pre-Phase-4/5 line, which never carried this token at all
        (watch_parallel_fix.py's tier-aware parser defaults an absent
        token to "T1", so every existing line stays valid).
        """
        def logging_call_model(messages, base_url, api_key, model, max_tokens, reasoning_effort,
                                stream=False, thinking=True, temperature=0, timeout=120,
                                max_retries=DEFAULT_MAX_RETRIES,
                                retry_backoff_seconds=DEFAULT_RETRY_BACKOFF_SECONDS,
                                max_retry_backoff_seconds=DEFAULT_MAX_RETRY_BACKOFF_SECONDS):
            ts = time.strftime("%Y-%m-%dT%H:%M:%S")
            prompt_chars = sum(len(m.get("content", "")) for m in messages)
            # worker_label in the filename (not just the manifest line):
            # req_log_dir is shared by every worker, and two workers
            # calling in the same second used to overwrite each other's
            # request/response artifacts. watch_context.py resolves the
            # worker-tagged name first and falls back to this legacy
            # shape for pre-existing files.
            req_path = req_log_dir / f"{ts}-{worker_label}-{phase}-request.json"
            req_path.write_text(json.dumps({
                "phase": phase, "model": model, "base_url": base_url, "max_tokens": max_tokens,
                "reasoning_effort": reasoning_effort, "stream": stream,
                "thinking": thinking, "temperature": temperature, "timeout": timeout,
                "deadline_seconds": config.get("deadline_seconds", DEFAULT_DEADLINE_SECONDS),
                "prompt_chars": prompt_chars, "messages": messages,
            }, indent=2))
            t0 = time.time()

            def log_retry(msg):
                # timestamped_log(msg) already shows this in the worker's plain
                # log (and hence watch_parallel_fix.py's dashboard); this also
                # appends a matching line to the structured manifest.log, which
                # previously only ever recorded this whole call's single final
                # outcome -- every individual 5xx/empty-reply retry riding out
                # inside call_model's own loop was invisible there.
                timestamped_log(msg)
                with req_manifest_path.open("a") as f:
                    f.write(
                        f"{time.strftime('%Y-%m-%dT%H:%M:%S')} phase={phase} worker={worker_label} "
                        f"tier={tier} provider={provider_slug(base_url)} model={model} RETRY {msg}\n"
                    )

            captured_usage = {}

            def capture_usage(usage):
                captured_usage["usage"] = usage

            try:
                reply = call_model(
                    messages, base_url, api_key, model, max_tokens, reasoning_effort,
                    stream, thinking, temperature, timeout,
                    max_retries, retry_backoff_seconds, max_retry_backoff_seconds,
                    log_fn=log_retry, usage_fn=capture_usage,
                    prompt_cache=config.get("prompt_cache", "auto"),
                    # The governor is account-global, so worker-vs-reviewer
                    # knob differences don't matter -- every phase shares the
                    # worker config's knobs and the one state file.
                    governor_path=DEFAULT_GOVERNOR_PATH,
                    governor_calls_per_minute=config["governor_calls_per_minute"],
                    governor_burst=config["governor_burst"],
                    deadline_seconds=config.get("deadline_seconds", DEFAULT_DEADLINE_SECONDS),
                    role=phase,
                    event_fn=append_call_event,
                )
            except Exception as e:
                elapsed = time.time() - t0
                with req_manifest_path.open("a") as f:
                    f.write(
                        f"{ts} phase={phase} worker={worker_label} tier={tier} "
                        f"provider={provider_slug(base_url)} model={model} "
                        f"prompt_chars={prompt_chars} elapsed={elapsed:.1f}s ERROR={e}\n"
                    )
                raise
            elapsed = time.time() - t0
            reply_path = req_log_dir / f"{ts}-{worker_label}-{phase}-response.txt"
            reply_path.write_text(reply)
            with req_manifest_path.open("a") as f:
                f.write(
                    f"{ts} phase={phase} worker={worker_label} tier={tier} "
                    f"provider={provider_slug(base_url)} model={model} "
                    f"prompt_chars={prompt_chars} elapsed={elapsed:.1f}s reply_chars={len(reply)} OK\n"
                )
            # Cache accounting goes to a separate cache-stats.log rather than
            # the manifest line, so the (dashboard-parsed) manifest format
            # stays byte-stable. One line per call the provider actually
            # reported usage for (see extract_cache_usage): a provider that
            # doesn't surface cache stats simply logs nothing here.
            cache_usage = extract_cache_usage(captured_usage.get("usage"))
            if cache_usage is not None:
                cached, total = cache_usage
                with cache_stats_path.open("a") as f:
                    f.write(
                        f"{ts} phase={phase} worker={worker_label} model={model} "
                        f"cached={cached} total={total}\n"
                    )
            return reply

        return logging_call_model

    logging_call_model_fixer = make_logging_call_model("fixer")
    logging_call_model_reviewer = make_logging_call_model("reviewer")
    logging_call_model_critique = make_logging_call_model("critique")

    prompt_log_dir = Path(args.prompt_log_dir)
    prompt_log_dir.mkdir(parents=True, exist_ok=True)
    prompt_log_path = prompt_log_dir / f"process-{worker_label}-prompt.log"

    tags_found_log_path = Path(args.tags_found_log)
    tags_found_log_path.parent.mkdir(parents=True, exist_ok=True)

    def log_tag_found(tag_gap, result):
        """Append one line to the shared tags-found log -- every worker in
        a parallel run points --tags-found-log at the same path, so this
        is a single running record of exactly which tags were found (and
        by whom, and when), not just each worker's own private log.
        Appends are small single lines (well under PIPE_BUF), so this is
        safe without extra locking even with multiple concurrent writers.

        tier=T2 for a clustered sibling-family landing, T1 otherwise --
        mirrors run_tag_loop's own claim-time tier stamp (spec section 5's
        KPI: watch_parallel_fix.py's tier_kpi_stats reads this field via
        parse_tags_found_log_tiered; a missing tier= token -- every
        pre-Phase-4/5 line -- defaults to "T1" there, so this is purely
        additive).
        """
        ts = time.strftime("%Y-%m-%dT%H:%M:%S")
        gaps_closed = result.get("gaps_closed", "?")
        tier = "T2" if tag_gap.get("cluster_members") else "T1"
        line = f"{ts} worker={worker_label} tag={tag_gap['tag_key']} gaps_closed={gaps_closed} tier={tier}\n"
        with tags_found_log_path.open("a") as f:
            f.write(line)
        total = sum(1 for _ in tags_found_log_path.open())
        timestamped_log(f"[{tag_gap['tag_key']}] logged to {tags_found_log_path} (total tags found so far: {total})")

    # Resolved once (cached -- see resolve_exiftool_perl_lib_dir) rather than
    # per-tag: None on a machine without exiftool on PATH, in which case
    # build_prompt's Perl-reference section is silently omitted, same as
    # samples_dir being unavailable.
    perl_lib_dir = resolve_exiftool_perl_lib_dir()

    # A missing/empty file (no verdicts logged yet, or none for this format)
    # is handled by load_recent_sweep_reviews itself -- always pass the
    # configured path rather than checking existence here first.
    sweep_review_log_path = Path(args.sweep_review_log)
    # Spec K1/K2/K3: the shared knowledge layer lives under OXIDEX_HOME
    # (GLOBAL-PITFALLS.md, knowledge/modules/*.md, logs/lessons.jsonl) --
    # the same fixed, worktree-independent home every other shared store
    # in this module already uses (rate-governor.json, landed-tags.log).
    # build_prompt's own module_name/table_name (K3 playbook selection,
    # M1 Table: trailer) still pass None below -- per-call module/table
    # resolution for THOSE call sites isn't wired; only run_tag_loop's
    # claim-time S4-item-5 cross-tier check (below) reads attribution.
    knowledge_home = OXIDEX_HOME

    # Spec S4 item 5: best-effort, read ONCE at startup (not re-read every
    # round -- attribution drifts slowly relative to a single process's
    # lifetime, and this is advisory only; see resolve_canonical_table's
    # own "never required" contract). None on a missing/corrupt file --
    # run_tag_loop's claim() closure then simply never excludes anything
    # on this basis, identical to every run before this feature existed.
    try:
        gap_attribution = json.loads(Path(args.gap_attribution).read_text())
    except (OSError, json.JSONDecodeError):
        gap_attribution = None

    def real_fix_tag(tag_gap, cfg, previous_attempts=None):
        fmt = tag_gap["format"]

        def current_match():
            """One fresh comparison for this tag's format -- used both
            as the M3 pre-attempt baseline (recheck_baseline, captured
            once before fix_gap's repair rounds begin) and, called
            again, by recheck() itself post-attempt each round ("read
            the tagcmp JSON before applying the diff" per spec M3).
            out_suffix keeps this out from under every other same-format
            process's feet -- see find_gaps_fn above. Threads the same
            build semaphore through as find_gaps_fn/cargo_*_fn above --
            this fires on every repair round's pre/post recheck, so it's
            exactly the high-frequency cargo invocation section 5's
            semaphore exists to gate."""
            path = run_format_comparison(
                fmt, args.cache_dir, out_suffix=worker_label,
                semaphore_path=DEFAULT_BUILD_SEMAPHORE_PATH,
                semaphore_max_holders=config["build_semaphore"],
            )
            regrouped = group_gaps_by_format(load_comparison_report(path))
            return next((g for g in regrouped if g["format"] == fmt), None)

        recheck_baseline = current_match()

        def recheck(_fmt):
            # _fmt is ignored -- tag_gap already knows its own format;
            # fix_gap's recheck_fn(format_name) contract is reused as-is,
            # scoped here to whether this ONE tag is still present rather
            # than the whole format's gap count.
            match = current_match()
            targets = [tag_gap] + list(tag_gap.get("cluster_members") or [])
            open_count, detail = 0, None
            for t in targets:
                st = tag_still_open(match, t)
                if st:
                    open_count += 1
                    if st[0] == "value_differs" and detail is None:
                        detail = (f'{t["tag_key"]}: present but wrong -- expected (exiftool): '
                                  f'"{st[1]}" / got (oxidex): "{st[2]}". Fix the value.')
                    elif st[0] == "duplicate_emission" and detail is None:
                        detail = (f'{t["tag_key"]}: duplicate emission detected (spec M3) -- '
                                  "the fix inserted a redundant second handler for this tag.")
            # 3-tuple (spec M3): the 3rd element is this round's raw
            # comparison dict, letting fix_gap classify wrong_value vs
            # structural (tag_still_open) and run the new_oxidex_only_keys
            # gate against recheck_baseline.
            return (open_count, detail, match)

        single_gap = make_cluster_gap(tag_gap)
        # Computed once per attempt (two git subprocess calls) and shared
        # by the preview and the real prompt below, so both show the
        # exact same precedent.
        precedent = build_neighbor_precedent_block(single_gap, REPO_ROOT)
        # Log the exact prompt this round is about to send -- to the
        # screen and to a per-worker file -- before the call goes out, so
        # "what is it sending" is visible immediately rather than only
        # reconstructable after the fact from logging_call_model's request
        # dump.
        prompt_preview = build_prompt(
            single_gap, repo_root=REPO_ROOT,
            # Mirror fix_gap's clustered max_tags so the preview shows
            # exactly what the model will actually receive.
            max_tags=(single_gap["gap_count"] if single_gap.get("clustered")
                      else cfg["max_prompt_tags"]),
            max_file_bytes=cfg["max_prompt_file_bytes"],
            samples_dir=Path(args.cache_dir) / "combined-samples",
            previous_attempts=previous_attempts,
            perl_lib_dir=perl_lib_dir,
            sweep_review_log_path=sweep_review_log_path,
            max_prompt_tokens=cfg.get("max_prompt_tokens", DEFAULT_MAX_PROMPT_TOKENS),
            max_reply_tokens=cfg.get("max_tokens", DEFAULT_MAX_TOKENS),
            neighbor_precedent_block=precedent,
            knowledge_home=knowledge_home, module_name=None,
            learning_budget_tokens=cfg.get("learning_budget_tokens", DEFAULT_LEARNING_BUDGET_TOKENS),
            parser_floor_tokens=cfg.get("parser_floor_tokens", DEFAULT_PARSER_FLOOR_TOKENS),
            lessons_tail_kb=cfg.get("lessons_tail_kb", DEFAULT_LESSONS_TAIL_KB),
            # The preview must mirror fix_gap's real prompt exactly --
            # the two per-worker learning sections included, or the log
            # would show a prompt the model never saw.
            worker_label=worker_label,
        )
        ts = time.strftime("%Y-%m-%dT%H:%M:%S")
        banner = f"\n{'=' * 20} [{ts}] worker={worker_label} tag={tag_gap['tag_key']} {'=' * 20}\n"
        print(banner + prompt_preview)
        with prompt_log_path.open("a") as f:
            f.write(banner + prompt_preview + "\n")

        result = fix_gap(
            single_gap, cfg, recheck_fn=recheck, recheck_baseline=recheck_baseline,
            review_config=review_config,
            git_apply_fn=logging_git_apply, log_fn=timestamped_log,
            call_model_fn=logging_call_model_fixer, review_call_model_fn=logging_call_model_reviewer,
            critique_call_model_fn=logging_call_model_critique,
            samples_dir=Path(args.cache_dir) / "combined-samples",
            previous_attempts=previous_attempts,
            perl_lib_dir=perl_lib_dir,
            sweep_review_log_path=sweep_review_log_path,
            neighbor_precedent_block=precedent,
            max_repair_rounds=cfg.get("max_repair_rounds", DEFAULT_MAX_REPAIR_ROUNDS),
            max_review_rounds=cfg.get("max_review_rounds", DEFAULT_MAX_REVIEW_ROUNDS),
            knowledge_home=knowledge_home, module_name=None, table_name=None,
            worker_label=worker_label,
            cargo_build_fn=cargo_build_fn, cargo_check_fn=cargo_check_fn,
            cargo_test_workspace_fn=cargo_test_workspace_fn, cargo_test_targeted_fn=cargo_test_targeted_fn,
        )
        if result["status"] == "fixed":
            log_tag_found(tag_gap, result)
        return result

    max_tags_per_process = (
        args.max_tags_per_process if args.max_tags_per_process is not None
        else (toml_data.get("parallel") or {}).get("max_tags_per_process", DEFAULT_MAX_TAGS_PER_PROCESS)
    )
    refresh_worktree_fn = (
        (lambda: refresh_worktree(REPO_ROOT, args.base_ref)) if args.base_ref else None
    )
    with worker_claim_lifecycle(args.tag_state_path, args.worker_id):
        summary = run_tag_loop(
            config, find_gaps_fn, real_fix_tag, state_path=args.tag_state_path,
            git_checkout_clean_fn=git_checkout_clean, repo_root=REPO_ROOT,
            log_fn=timestamped_log, max_fails=args.max_tag_fails,
            blacklist_full=args.blacklist_full, worker_id=args.worker_id,
            max_distinct_tags=max_tags_per_process, refresh_worktree_fn=refresh_worktree_fn,
            max_cluster_tags=config["max_cluster_tags"],
            landed_tags_path=DEFAULT_LANDED_TAGS_PATH,
            claim_stale_seconds=config["claim_stale_seconds"],
            heartbeat_seconds=config["heartbeat_seconds"],
            attribution=gap_attribution,
            # Pre-dispatch reachability: one oxidex run per FORMAT (memoized),
            # against that format's own sample, before any model call is spent
            # on it. A format whose parser never executes -- ISO 9660 on
            # 2026-07-30, whose signature sits at byte 32769 while detection
            # buffers 1 KiB -- is skipped with a reason instead of retried.
            reachability_fn=make_reachability_fn(REPO_ROOT, log_fn=timestamped_log),
        )
    print(f"stopped after {summary['rounds']} rounds")
    print(f"  fixed:   {len(summary['fixed'])} tags")
    print(f"  failed:  {len(summary['failed'])} attempts")
    # "skipped" now carries two distinct kinds: status "duplicate" (already
    # fixed elsewhere) and status "unreachable" (the pre-dispatch gate --
    # zero model calls spent). Counting them together would hide exactly
    # the number this gate exists to make visible.
    unreachable = [s for s in summary["skipped"] if s.get("status") == "unreachable"]
    print(f"  skipped: {len(summary['skipped']) - len(unreachable)} tags (already fixed elsewhere)")
    print(f"  unreachable: {len(unreachable)} tags (format parser never runs -- no calls spent)")
    print(f"  cycles reset (blacklist exhausted): {summary['cycles_reset']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
