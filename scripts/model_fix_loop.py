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
    max_tokens           default 4096 (cap on the model's own reply length)
    max_prompt_tokens     default 8192 (worker only; hard cap on the built
                          prompt itself -- see estimate_tokens/
                          assemble_prompt_sections -- a ~4 chars/token
                          estimate, no real tokenizer dependency. Overflow
                          is shed via graduated per-section truncation
                          (attempts, then samples, then neighbor precedent,
                          then perl_block, then parser files down to
                          parser_floor_tokens), not plain head-keeping. If
                          the model's own diff would exceed this in one
                          reply, the prompt tells it to split into
                          "PATCH i/N" chunks instead -- see attempt_build.)
    reviewer_max_prompt_tokens default 8192 (reviewer only; independent cap
                          on build_review_prompt's own prompt, which now
                          carries the Perl reference, live post-fix
                          evidence, and a scoped emission scan alongside
                          the C1-C5 checklist -- see review_verdict)
    learning_budget_tokens default 1200 (worker only; flat, reserved token
                          budget for the learning block -- GLOBAL-PITFALLS.md
                          excerpt + module playbook + sweep reviews +
                          lessons tail -- never squeezed further and never
                          dropped entirely, see build_prompt)
    parser_floor_tokens    default 2000 (worker only; the parser-files
                          section never shrinks below this even under the
                          worst prompt overflow -- "elastic with a floor")
    lessons_tail_kb        default 256 (worker only; how far back
                          build_prompt seeks into the tail of
                          logs/lessons.jsonl -- bounded, no full scan of a
                          ledger that only grows -- see read_lessons_tail)
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
    max_request_turns      default 20 (worker only; how many REQUEST:
                          <path> investigation turns -- see
                          attempt_build/resolve_request -- the fixer gets
                          before it's nudged, then required, to submit a
                          diff instead of continuing to investigate)
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
    governor_cooldown_seconds default 30 (base GLOBAL cooldown one
                          rate-limited call (429/5xx) imposes on the
                          whole fleet via governor_report; doubles per
                          consecutive limited outcome)
    governor_max_cooldown_seconds default 300 (cap on that exponential
                          cooldown growth)
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
import fcntl
import functools
import json
import os
import random
import re
import shutil
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import tempfile
import threading
import time
import tomllib
import urllib.error
import urllib.request
from pathlib import Path

from find_tag_gaps import (
    OXIDEX_HOME,
    REPO_ROOT,
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
    make_lesson as make_lesson_event,
)

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


def extract_diff(response_text):
    """Pull a unified diff out of a chat response.

    Prefers a fenced ```diff block; falls back to treating the whole
    response as a diff if it looks like one (starts with "diff --git" or
    "--- "). Returns None if nothing diff-shaped is found.
    """
    match = DIFF_BLOCK_RE.search(response_text)
    if match:
        return strip_patch_sentinels(match.group(1))
    stripped = response_text.strip()
    if stripped.startswith("diff --git") or stripped.startswith("--- "):
        return strip_patch_sentinels(stripped)
    return None


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


# 429 is retryable now that the governor paces retries fleet-wide (see
# governor_acquire/governor_report) instead of each process backing off
# independently against the shared account limit.
DEFAULT_RETRYABLE_HTTP_STATUSES = {429, 500, 502, 503, 504}
DEFAULT_MAX_RETRIES = 1000
DEFAULT_RETRY_BACKOFF_SECONDS = 2
DEFAULT_MAX_RETRY_BACKOFF_SECONDS = 120  # cap growth -- 2**1000 would otherwise be absurd


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
                governor_cooldown_seconds=None, governor_max_cooldown_seconds=None):
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

    governor_path (None disables, keeping every old caller byte-identical
    in behavior) points at the cross-process rate-governor state file (see
    governor_acquire/governor_report): every attempt first acquires one
    governor slot (waiting out the shared token bucket and any fleet-wide
    cooldown, reusing this call's sleep_fn), and every outcome is reported
    back -- limited=True for a retryable HTTPError (429/5xx, which
    sets/extends the GLOBAL cooldown so one limited worker pauses the
    whole fleet), limited=False for a success or a connection-level
    URLError (infrastructure being unreachable is not rate limiting). The
    governor_* knobs default to None, resolved to the DEFAULT_GOVERNOR_*
    values at call time (they are defined later in this module).

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
    governor_cooldown_seconds = (
        DEFAULT_GOVERNOR_COOLDOWN_SECONDS if governor_cooldown_seconds is None
        else governor_cooldown_seconds
    )
    governor_max_cooldown_seconds = (
        DEFAULT_GOVERNOR_MAX_COOLDOWN_SECONDS if governor_max_cooldown_seconds is None
        else governor_max_cooldown_seconds
    )
    last_error = None
    for attempt in range(max_retries + 1):
        if attempt > 0:
            delay = min(retry_backoff_seconds * (2 ** (attempt - 1)), max_retry_backoff_seconds)
            if log_fn:
                log_fn(
                    f"model call retry {attempt}/{max_retries} after {last_error!r}, "
                    f"waiting {delay}s"
                )
            sleep_fn(delay)
        governor_acquire(governor_path, governor_calls_per_minute, governor_burst,
                         sleep_fn=sleep_fn)
        try:
            reply, usage = _call_model_once(
                messages, base_url, api_key, model, max_tokens, reasoning_effort,
                stream, thinking, temperature, timeout, prompt_cache,
            )
        except urllib.error.HTTPError as e:
            governor_report(governor_path, limited=(e.code in DEFAULT_RETRYABLE_HTTP_STATUSES),
                            cooldown_seconds=governor_cooldown_seconds,
                            max_cooldown_seconds=governor_max_cooldown_seconds)
            if e.code not in DEFAULT_RETRYABLE_HTTP_STATUSES:
                raise
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
            # Connection failures aren't rate limiting: limited=False.
            governor_report(governor_path, limited=False,
                            cooldown_seconds=governor_cooldown_seconds,
                            max_cooldown_seconds=governor_max_cooldown_seconds)
            last_error = e
            continue
        if not reply:
            last_error = last_error or RuntimeError("model returned an empty reply")
            continue
        if usage_fn is not None:
            usage_fn(usage)
        governor_report(governor_path, limited=False,
                        cooldown_seconds=governor_cooldown_seconds,
                        max_cooldown_seconds=governor_max_cooldown_seconds)
        return reply
    # last_error is only None if max_retries < 0 (range(max_retries + 1) never
    # iterates) -- guard against `raise None`, which would raise a confusing
    # TypeError instead of surfacing the actual misconfiguration.
    raise last_error or RuntimeError("call_model: max_retries < 0, no attempt was made")


def _call_model_once(messages, base_url, api_key, model, max_tokens, reasoning_effort, stream, thinking,
                      temperature, timeout, prompt_cache="auto"):
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
    # base_url is developer-supplied local config (MODEL_FIX_BASE_URL /
    # REVIEW_BASE_URL), never network- or attacker-controlled input.
    with urllib.request.urlopen(req, timeout=timeout) as resp:  # nosec B310
        if not stream:
            response = json.loads(resp.read())
            return response["choices"][0]["message"]["content"], response.get("usage")

        chunks = []
        usage = None
        for raw_line in resp:
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


def git_apply(diff_text, repo_root):
    """Apply a unified diff to the working tree. Returns (success, message).

    List-argv only, no shell=True anywhere in this file -- repo_root is a
    local path this process already trusts (the repo it's running in), and
    diff_text is passed via stdin, never interpolated into the argv list.

    --recount tells git to ignore each hunk's stated @@ -a,b +c,d @@ line
    counts and recompute them from the actual +/-/context lines instead --
    models routinely emit diffs with an off-by-one in that header despite
    otherwise-correct content, which git rejects outright as "corrupt
    patch" without this flag. Harmless for a diff whose counts were
    already right.
    """
    result = subprocess.run(  # nosec B603
        ["git", "apply", "--reject", "--recount", "-"],
        input=diff_text, capture_output=True, text=True, cwd=repo_root,
    )
    if result.returncode == 0:
        return True, "applied"
    return False, result.stderr


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
    """
    result = subprocess.run(  # nosec B603
        ["git", "merge", "--ff-only", base_ref],
        cwd=repo_root, capture_output=True, text=True,
    )
    return result.returncode == 0, (result.stdout + result.stderr).strip()


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
DEFAULT_GOVERNOR_COOLDOWN_SECONDS = 30
DEFAULT_GOVERNOR_MAX_COOLDOWN_SECONDS = 300


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
        state.setdefault("cooldown_until", 0.0)
        state.setdefault("consecutive_limited", 0)
        new_state, result = mutate_fn(state)
        path.write_text(json.dumps(new_state))
        return result


def governor_acquire(path, calls_per_minute=DEFAULT_GOVERNOR_CALLS_PER_MINUTE,
                     burst=DEFAULT_GOVERNOR_BURST, now_fn=time.time,
                     sleep_fn=time.sleep, jitter_fn=random.random):
    """Block until this process may make one model API call.

    Cross-process token bucket + global cooldown, shared by every worker
    through one flock-guarded JSON file: refill at calls_per_minute/60
    tokens/sec (capped at burst), spend one per call, and honor
    cooldown_until -- which governor_report sets GLOBALLY on a 429/5xx,
    so one worker being limited pauses the whole fleet instead of the
    other N-1 continuing to hammer the shared account limit (measured
    today: 20 independent backoffs -> 13k 429s and zero successes in an
    hour). Waits carry +/-20% jitter so workers don't all wake at the
    same instant. path=None disables (old callers, tests).
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
            if now < state["cooldown_until"]:
                return state, state["cooldown_until"] - now
            if state["tokens"] < 1.0:
                return state, (1.0 - state["tokens"]) / rate
            state["tokens"] -= 1.0
            return state, None

        wait = _governor_locked(path, try_take, now_fn)
        if wait is None:
            return
        sleep_fn(wait * (0.8 + 0.4 * jitter_fn()))


def governor_report(path, limited, cooldown_seconds=DEFAULT_GOVERNOR_COOLDOWN_SECONDS,
                    max_cooldown_seconds=DEFAULT_GOVERNOR_MAX_COOLDOWN_SECONDS,
                    now_fn=time.time):
    """Record one call outcome. limited=True (429 or 5xx) sets/extends
    the GLOBAL cooldown with exponential growth per consecutive limited
    outcome, capped; limited=False resets the streak (the next limited
    outcome starts from the base cooldown again). path=None disables."""
    if path is None:
        return
    def mutate(state):
        if limited:
            state["consecutive_limited"] += 1
            backoff = min(
                cooldown_seconds * (2 ** (state["consecutive_limited"] - 1)),
                max_cooldown_seconds,
            )
            state["cooldown_until"] = max(state["cooldown_until"], now_fn() + backoff)
        else:
            state["consecutive_limited"] = 0
        return state, None
    _governor_locked(path, mutate, now_fn)


def cargo_build(repo_root):
    """Build the oxidex binary to verify a candidate diff compiles.

    Uses the "fixloop" profile (see Cargo.toml) rather than --release --
    this is a correctness check, not a binary anyone ships, so it isn't
    worth paying release's fat-LTO/single-codegen-unit compile cost on
    every single verification build.

    Returns (success, stderr).
    """
    result = subprocess.run(  # nosec B603
        ["cargo", "build", "--profile", "fixloop", "--bin", "oxidex"],
        capture_output=True, text=True, cwd=repo_root, env=cargo_env(),
    )
    return result.returncode == 0, result.stderr


def cargo_check(repo_root):
    """Fast compile-only check (no codegen, no tests) for VERIFY trial
    diffs -- see attempt_build. Returns (success, output), stdout+stderr
    combined (cargo check's errors go to stderr, but warnings/summaries
    can land on stdout)."""
    result = subprocess.run(  # nosec B603
        ["cargo", "check", "--workspace"],
        capture_output=True, text=True, cwd=repo_root, env=cargo_env(),
    )
    return result.returncode == 0, result.stdout + result.stderr


def cargo_test_workspace(repo_root):
    """Run the full workspace test suite. Returns (success, output) --
    output is stdout+stderr combined (cargo test's failure detail --
    which assertion failed, panic message, etc. -- goes to stdout, not
    stderr, unlike cargo build's compiler errors), so a caller can feed
    the actual failure back to the model instead of just "tests
    regressed" with no detail to act on."""
    result = subprocess.run(  # nosec B603
        ["cargo", "test", "--workspace"],
        capture_output=True, text=True, cwd=repo_root, env=cargo_env(),
    )
    return result.returncode == 0, result.stdout + result.stderr


def cargo_test_targeted(repo_root, filter_str):
    """Fast first-line test gate: only lib tests whose names match
    filter_str (the format lowercased -- best-effort, zero matches is a
    pass, which cargo already treats as success). The full workspace
    suite still gates every commit; this just stops candidates that are
    about to die at review from paying the full-suite price first."""
    result = subprocess.run(  # nosec B603
        ["cargo", "test", "--lib", filter_str],
        capture_output=True, text=True, cwd=repo_root, env=cargo_env(),
    )
    return result.returncode == 0, result.stdout + result.stderr


DEFAULT_MAX_PROMPT_TAGS = 40
DEFAULT_MAX_PROMPT_FILE_BYTES = 60_000


DEFAULT_MAX_SAMPLE_FILES_LISTED = 15


DEFAULT_MAX_ATTEMPT_DIFF_CHARS = 2000


def format_previous_attempts(previous_attempts, max_diff_chars=DEFAULT_MAX_ATTEMPT_DIFF_CHARS):
    """Render a tag's attempt history (see run_tag_loop's persisted
    per-tag "attempts" list) into a prompt section, so a later round gets
    to see what earlier rounds already tried and why it failed instead of
    repeating the same broken approach from scratch. Each diff is
    truncated -- the point is "what direction was tried", not a byte-exact
    replay -- so this stays bounded even after many rounds' worth of
    history accumulates for one stubborn tag."""
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
        "repeat the same broken approach):\n\n" + "\n\n".join(blocks)
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

    The bare system perl doesn't have Image::ExifTool installed as a
    regular module -- exiftool bundles its own copy and adds it to @INC
    itself -- so this can't just `use Image::ExifTool` and read $INC.
    Homebrew's exiftool formula patches the exact lib path directly into
    the installed script instead (an `unshift @INC, "<path>/lib/perl5"`
    line added to its BEGIN block), so this reads it straight from there:
    works for whatever exiftool is actually on PATH, on whatever machine,
    without hardcoding a version-specific Cellar path that breaks on the
    next `brew upgrade`.
    """
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


def build_format_overview_block(lib_dir, perl_reference_block):
    """Combine ExifTool's own NOTES documentation for this gap's relevant
    Perl module(s) -- extracted from whichever files build_perl_reference_block
    already found tags in, so no redundant module discovery -- with a
    short, always-included primer on how oxidex's own parsers are
    organized. The per-tag Perl snippets and the full parser file
    contents (see build_prompt's other sections) already show the
    specifics; this section is deliberately just orientation."""
    notes_blocks = []
    if lib_dir is not None:
        module_table_pairs = sorted(set(PERL_MODULE_HEADER_RE.findall(perl_reference_block)))
        for name, table_name in module_table_pairs:
            notes = extract_perl_table_notes(lib_dir / name, table_name=table_name or None)
            if notes:
                label = f"{name}, table {table_name}" if table_name else name
                notes_blocks.append(f"--- {label} ---\n{notes}")

    notes_section = ""
    if notes_blocks:
        notes_section = (
            "\n\nExifTool's own documentation for this format (from the Perl source's NOTES):\n\n"
            + "\n\n".join(notes_blocks)
        )

    return f"\n\n{ARCHITECTURE_PRIMER}{notes_section}"


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


def build_reply_shape_manifest(max_prompt_tokens):
    """The complete reply protocol, stated once near the top of the
    prompt (stable text -> provider prompt-cache friendly; early text ->
    survives truncate_to_token_budget, which keeps the head)."""
    return f"""You are operating in an ephemeral, isolated git worktree; broken builds during investigation are expected and cost nothing -- probe aggressively with VERIFY rather than guessing.

Every reply must be exactly one of these four shapes:

1. REQUEST: <path> -- see a source file or a sample file (a bare line, nothing else in the reply). Add :<start>-<end> after a source path (e.g. REQUEST: src/parsers/x.rs:40-120) to get just that 1-indexed line range -- prefer a range for anything large.
2. VERIFY -- trial-compile a candidate change without committing to it: the line "VERIFY" followed by exactly ONE ```diff fenced block. The diff is applied, `cargo check` runs, the tail of its output comes back, and the change is REVERTED -- your final diff must still contain the complete change.
3. PATCH 1/N -- if your finished diff would exceed roughly {max_prompt_tokens} tokens (~{max_prompt_tokens * 4} characters) in one reply, split it into N consecutive chunks and send the first as the line "PATCH 1/N" followed by ONE ```diff fenced chunk; you'll be prompted for each next chunk. Chunks are concatenated in order before applying, so split anywhere (mid-hunk is fine) -- never repeat or skip lines across a boundary.
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
            f"\n\nReal sample file containing this exact tag ({shown_path}, {size} bytes) "
            f"-- full hex dump:\n{hex_dump(data, max_bytes=DEFAULT_INLINE_SAMPLE_MAX_BYTES)}"
        )
    return (
        f"\n\nReal sample file containing this exact tag: {shown_path} ({size} bytes, too "
        f"large to inline here). Respond with \"REQUEST: {shown_path}\" instead of a diff if "
        "you need to see its raw bytes."
    )


# --- Section 6: lessons tail (part of the learning block) ------------------

DEFAULT_LESSONS_TAIL_KB = 256
DEFAULT_LESSONS_TAIL_MAX_ENTRIES = 8


def read_lessons_tail(lessons_path, module_key, format_name,
                       tail_kb=DEFAULT_LESSONS_TAIL_KB,
                       max_entries=DEFAULT_LESSONS_TAIL_MAX_ENTRIES):
    """Section 6: the last `tail_kb` KB of <home>/logs/lessons.jsonl via
    seek (bounded, no full scan of a ledger that only ever grows), non-
    infra events filtered to the same module (when module_key is given)
    else the same format, newest `max_entries` kept.

    A byte offset can land mid-line (either the seek itself, or a writer
    mid-append at the moment of the read) -- the first split chunk after
    a nonzero offset is dropped rather than risking a truncated JSON
    object; every other malformed line is skipped the same way every
    other K1 reader skips one (never degrades to {}). Missing file (not
    yet created, or no lessons_path given): []."""
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
        if not event or event == "infra":
            continue
        if module_key:
            if ev.get("module") != module_key:
                continue
        elif ev.get("format") != format_name:
            continue
        events.append(ev)
    events.reverse()  # ledger is append-order (oldest first); newest first
    return events[:max_entries]


def format_lessons_tail(events):
    """Render read_lessons_tail's output into a learning-block section."""
    if not events:
        return ""
    lines = []
    for ev in events:
        event = ev.get("event", "?")
        reason = str(ev.get("reason") or "").strip()
        tag_key = ev.get("tag_key") or ""
        suffix = f" ({tag_key})" if tag_key else ""
        lines.append(f"  - {event}: {reason}{suffix}")
    return (
        "\n\nRecent lessons ledger entries (fleet-wide, spec K1 -- other "
        "workers' recent outcomes on this module/format):\n"
        + "\n".join(lines)
    )


def build_prompt(gap, repo_root=REPO_ROOT, max_tags=DEFAULT_MAX_PROMPT_TAGS,
                  max_file_bytes=DEFAULT_MAX_PROMPT_FILE_BYTES, samples_dir=None,
                  max_samples_listed=DEFAULT_MAX_SAMPLE_FILES_LISTED, previous_attempts=None,
                  perl_lib_dir=None, sweep_review_log_path=None,
                  max_prompt_tokens=DEFAULT_MAX_PROMPT_TOKENS,
                  neighbor_precedent_block="",
                  knowledge_home=None, module_name=None,
                  learning_budget_tokens=DEFAULT_LEARNING_BUDGET_TOKENS,
                  parser_floor_tokens=DEFAULT_PARSER_FLOOR_TOKENS,
                  lessons_tail_kb=DEFAULT_LESSONS_TAIL_KB):
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
    format match -- see read_lessons_tail). Has no effect when
    knowledge_home is None.

    learning_budget_tokens/parser_floor_tokens/lessons_tail_kb are the
    section-6 knobs: the learning block (pitfalls excerpt + module
    playbook + sweep reviews + lessons tail -- everything knowledge_home
    gates, plus sweep_review_log_path's section) is capped at
    learning_budget_tokens and never dropped entirely; the parser-files
    section never shrinks below parser_floor_tokens even under the worst
    overflow; lessons_tail_kb bounds how much of the tail of
    logs/lessons.jsonl read_lessons_tail seeks into.

    neighbor_precedent_block is a pre-rendered string (see
    build_neighbor_precedent_block, built by the caller so build_prompt
    itself stays free of subprocess calls) inserted after the Perl
    reference in the stable per-tag section; "" (the default) omits it.

    Sections are ordered static-first (constraints/pitfalls/manifest),
    then per-tag content, then volatile history, so the byte-stable
    prefix is maximal for provider prompt caching. Section 6: overflow
    beyond max_prompt_tokens is shed via graduated per-section truncation
    (see assemble_prompt_sections) rather than plain head-keeping --
    attempts, then samples, then neighbor precedent, then perl_block,
    then the parser-files section down to (never below)
    parser_floor_tokens, in that priority order; the learning block is
    never part of that squeeze (it gets its own flat, never-emptied
    budget instead).
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

    overview_block = build_format_overview_block(perl_lib_dir, perl_block)

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

    # Section 6: lessons tail, also part of the learning block.
    lessons_tail_block = ""
    if knowledge_home is not None:
        lessons_tail_block = format_lessons_tail(read_lessons_tail(
            Path(knowledge_home) / "logs" / "lessons.jsonl",
            module_name, gap["format"], tail_kb=lessons_tail_kb,
        ))

    # Spec section 6: the learning block (pitfalls excerpt sits in the
    # static prefix above for cache-prefix reasons -- see the docstring's
    # "Section order otherwise unchanged" note; only the remaining three
    # pieces share this reserved, never-emptied budget) is capped flat,
    # independent of whatever else is squeezing the rest of the prompt.
    learning_text = _clamp_section_tokens(
        sweep_review_block + module_block + lessons_tail_block, learning_budget_tokens,
    )

    attempts_block = format_previous_attempts(previous_attempts)

    manifest = build_reply_shape_manifest(max_prompt_tokens)
    sections = [
        ("intro", (
            f"{RUST_ARCHITECTURE_CONSTRAINTS}\n\n"
            f"You are fixing ExifTool tag-coverage gaps in the oxidex Rust codebase, format \"{gap['format']}\".\n\n"
            f"{pitfalls_text}\n\n"
            f"{manifest}\n\n"
            f"Missing entirely (ExifTool extracts it, oxidex doesn't):\n{missing}\n\n"
        )),
        ("gaps", f"Value differences (both extract it, values disagree):\n{diffs}"),
        ("overview", f"{overview_block}\n\nLikely relevant source files:\n"),
        ("parser_files", files),
        ("samples", samples_block),
        ("exact_sample", exact_sample_block),
        ("perl_block", perl_block),
        ("neighbor", neighbor_precedent_block),
        ("learning", learning_text),
        ("attempts", attempts_block),
        ("tail", (
            "\n\nFor value differences, only fix genuine bugs, not benign formatting differences. "
            "If more gaps exist than are shown above, that's expected -- fix what's shown here; "
            "future rounds will address the rest.\n\n"
            f"{TERMINAL_REMINDER}"
        )),
    ]
    # Section 6 shrink-priority order: attempts, then samples, then
    # neighbor precedent, then perl_block, then parser files down to
    # (never below) parser_floor_tokens. "learning" is deliberately
    # absent -- its own flat cap above already gives it the "reserved,
    # never dropped entirely" guarantee independent of this squeeze.
    budgets = {
        "attempts": 0,
        "samples": 0,
        "neighbor": 0,
        "perl_block": 0,
        "parser_files": parser_floor_tokens,
    }
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
    if stripped.upper().startswith("APPROVE"):
        return "approve", ""
    if stripped.upper().startswith("UNVERIFIABLE"):
        _, _, reason = stripped.partition(":")
        return "unverifiable", reason.strip() or "unverifiable, no checklist id given"
    if stripped.upper().startswith("REJECT"):
        _, _, reason = stripped.partition(":")
        return "reject", reason.strip() or "rejected, no reason given"
    return "reject", f"unparseable review verdict: {stripped[:200]!r}"


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


REQUEST_RE = re.compile(r"^REQUEST:\s*(.+)$", re.IGNORECASE)
DEFAULT_MAX_REQUEST_TURNS = 20  # investigation turns before a diff is still required
DEFAULT_MAX_REQUEST_REPEATS = 3  # identical REQUESTs before a pivot nudge replaces the content
DEFAULT_HEXDUMP_BYTES = 2048

PATCH_HEADER_RE = re.compile(r"^PATCH\s+(\d+)\s*/\s*(\d+)\b", re.IGNORECASE)
DEFAULT_MAX_PATCH_CHUNKS = 40  # hard safety cap, independent of the declared N -- a
# misbehaving/looping model must not be able to stall an attempt forever

VERIFY_RE = re.compile(r"^VERIFY\b", re.IGNORECASE)
DEFAULT_MAX_VERIFY_TURNS = 10   # trial-compile turns per attempt_build invocation
DEFAULT_MAX_CHECK_OUTPUT_CHARS = 3000  # tail-trim: Rust errors summarize at the end


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


def parse_request_range(path_str):
    """Split a "path:START-END" request into (path, start, end).

    Returns (path, None, None) when there's no numeric range suffix. A
    range-shaped suffix with start < 1 or start > end strips the suffix
    but returns no range -- whole-file fallback -- rather than failing
    the entire request over a typo'd range. A non-numeric suffix (e.g.
    "x.rs:a-b") isn't range-shaped at all, so it stays part of the path
    and fails resolution with the normal could-not-resolve message.
    """
    stripped = path_str.strip()
    m = REQUEST_RANGE_RE.match(stripped)
    if not m:
        return stripped, None, None
    start, end = int(m.group(2)), int(m.group(3))
    if start < 1 or end < start:
        return m.group(1), None, None
    return m.group(1), start, end


def resolve_request(path_str, repo_root, samples_dir, max_text_bytes=20_000):
    """Answer a model's "REQUEST: <path>" turn -- a hex dump if the path
    resolves under samples_dir (real binary sample data), the raw text if
    it resolves under repo_root (more source to read), or a rejection
    message otherwise. Path traversal outside both roots is refused.
    A "path:START-END" suffix on a source file returns just that 1-indexed
    inclusive line range, numbered; samples always get the whole-file hex
    dump.
    """
    path_part, range_start, range_end = parse_request_range(path_str)
    candidates = []
    if samples_dir is not None:
        candidates.append((Path(samples_dir) / path_part, "sample"))
    candidates.append((repo_root / path_part, "source"))

    for candidate, kind in candidates:
        try:
            resolved = candidate.resolve()
        except OSError:
            continue
        root = (Path(samples_dir).resolve() if kind == "sample" else repo_root.resolve())
        if root not in resolved.parents and resolved != root:
            continue
        if not resolved.is_file():
            continue
        if kind == "sample":
            data = resolved.read_bytes()
            return (
                f"Hex dump of {path_part} ({len(data)} bytes total, "
                f"showing first {min(len(data), DEFAULT_HEXDUMP_BYTES)}):\n"
                f"{hex_dump(data)}"
            )
        content = resolved.read_text(errors="replace")
        if range_start is not None:
            lines = content.splitlines()
            if range_start > len(lines):
                return (
                    f"{path_part} has only {len(lines)} lines -- the requested range "
                    f"{range_start}-{range_end} starts past the end. Request a range within the file."
                )
            clamped_end = min(range_end, len(lines))
            numbered = "\n".join(
                f"{i}: {line}"
                for i, line in enumerate(lines[range_start - 1:clamped_end], start=range_start)
            )
            return f"Lines {range_start}-{clamped_end} of {path_part}:\n{numbered}"
        return f"Contents of {path_part}:\n{content[:max_text_bytes]}"

    return f"Could not resolve {path_part!r} under the samples dir or repo root -- try a path from the list shown."


def attempt_build(messages, *, call_model_fn, git_apply_fn, git_checkout_clean_fn,
                   cargo_build_fn, config, repo_root, pick_model_fn=random.choice,
                   samples_dir=None, cargo_check_fn=None):
    """Try to get a working build via a bounded conversation: up to
    config["max_request_turns"] turns where the model can ask to see more
    context (REQUEST: <path> -- see resolve_request) before it must submit
    a diff, then up to 2 diff attempts (initial + one apply/build repair
    round-trip). Extends the given messages conversation in place. Returns
    (built, reason, diff, messages) -- reason is None when built is True;
    diff is the successfully-applied diff (None if not built).

    pick_model_fn(models) -> model_spec is called fresh before every
    individual model call (not once per attempt_build invocation), so a
    repair round-trip can land on a different model -- potentially a
    different provider entirely -- from config["models"] than the initial
    attempt. Each spec is a {"name", "base_url", "api_key"} dict.

    A single reply is capped at config["max_tokens"] (see build_prompt's
    matching config["max_prompt_tokens"] on the request side), so a large
    diff may not fit in one turn. build_prompt tells the model to split
    such a diff into "PATCH i/N" chunks instead of truncating it silently
    -- see PATCH_HEADER_RE below. Each chunk is accumulated (up to
    DEFAULT_MAX_PATCH_CHUNKS turns, a safety ceiling independent of N, in
    case of a misbehaving/looping model) and, once chunk N/N arrives with
    every chunk 1..N present, concatenated back into one diff and applied
    exactly like a normal single-reply diff -- this doesn't consume a
    separate diff_attempts_used slot per chunk, only once the full diff is
    assembled and ready to apply.

    cargo_check_fn(repo_root) -> (success, output), if provided, enables
    the VERIFY protocol: a reply of "VERIFY" plus one ```diff fenced
    block gets that diff applied, cargo-checked, REVERTED, and the
    tail-trimmed check output fed back -- a trial compile that never
    consumes one of the 2 real diff attempts. Bounded by
    config["max_verify_turns"] (default DEFAULT_MAX_VERIFY_TURNS).
    None (the default) keeps VERIFY off: such replies get an
    "unavailable" message, so old callers and tests are unaffected.
    """
    max_request_turns = config.get("max_request_turns", DEFAULT_MAX_REQUEST_TURNS)
    request_turns_used = 0
    max_request_repeats = config.get("max_request_repeats", DEFAULT_MAX_REQUEST_REPEATS)
    request_counts = {}
    max_verify_turns = config.get("max_verify_turns", DEFAULT_MAX_VERIFY_TURNS)
    verify_turns_used = 0
    verify_rejections = 0
    diff_attempts_used = 0
    nudged_to_stop_investigating = False
    patch_chunks = {}
    patch_turns_used = 0
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
            # Network/timeout/HTTP/malformed-response failures are a normal
            # cost of "any model" -- a single bad call must not kill the
            # whole loop. No repair round-trip here: retrying the same
            # oversized/slow request immediately is unlikely to help; the
            # cross-round 2-strikes skip-list is what handles this format
            # long-term if it keeps failing.
            return False, f"model call failed: {e}", None, messages

        messages.append({"role": "assistant", "content": reply})

        request_match = REQUEST_RE.match(reply.strip())
        if request_match:
            normalized = request_match.group(1).strip()
            request_counts[normalized] = request_counts.get(normalized, 0) + 1
            if request_turns_used < max_request_turns:
                request_turns_used += 1
                if request_counts[normalized] >= max_request_repeats:
                    # Dead-end: the same path over and over. Re-serving
                    # identical content burns budget without advancing
                    # anything -- course-correct instead.
                    messages.append({
                        "role": "user",
                        "content": (
                            f"You've now requested {normalized!r} {request_counts[normalized]} times -- "
                            "it was already provided in full and re-reading it will not change anything. "
                            "Pivot: request a DIFFERENT file, narrow to a line range "
                            "(REQUEST: path:START-END), or submit your best diff now."
                        ),
                    })
                    current_phase = "patch"
                else:
                    answer = resolve_request(request_match.group(1), repo_root, samples_dir)
                    messages.append({"role": "user", "content": answer})
                    current_phase = "explore"
                continue
            if not nudged_to_stop_investigating:
                # Previously fell straight through to extract_diff on this
                # same REQUEST-shaped reply and failed immediately with "no
                # diff in model response" -- silently wasting the whole
                # attempt on investigation without ever telling the model
                # to actually submit something. One explicit nudge first.
                nudged_to_stop_investigating = True
                messages.append({
                    "role": "user",
                    "content": (
                        "You've used all your allowed investigation turns for this attempt. "
                        "No more file requests -- submit your best diff now (in a ```diff "
                        "fenced block) based on what you've already seen, even if you're not "
                        "fully certain."
                    ),
                })
                current_phase = "patch"
                continue
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
            diff = extract_diff(reply)
            if diff is None:
                return False, "no diff in model response", None, messages

        diff_attempts_used += 1
        applied, apply_msg = git_apply_fn(diff, repo_root)
        if not applied:
            git_checkout_clean_fn(repo_root)
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
        patch_chunks = {}
        messages.append({
            "role": "user",
            "content": f"The build failed:\n{build_err}\nPlease resend a corrected diff.",
        })
        current_phase = "patch"

    return False, "no working fix after repair attempt", None, messages


DEFAULT_MAX_REPAIR_ROUNDS = 5
DEFAULT_MAX_TEST_OUTPUT_CHARS = 3000

# attempt_build's exception path is the single producer of this prefix
# (its `return False, f"model call failed: {e}", ...`); fix_gap and
# run_tag_loop both key off it to recognize infrastructure
# (rate-limit/network/provider) failures that say nothing about the tag
# or the diff.
INFRA_FAILURE_PREFIX = "model call failed:"


# --- K5: reviewer evidence defaults -----------------------------------------

def default_extract_live_evidence(repo_root, sample_path, tag_keys):
    """K5 real default for fix_gap's extract_evidence_fn: shell out to
    exiftool and this worktree's own oxidex binary for just the target
    tags on one real sample file -- NOT the comparison JSON (whose
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
    binary = next(
        (c for c in (repo_root / "target" / "debug" / "oxidex",
                     repo_root / "target" / "fixloop" / "oxidex") if c.is_file()),
        None,
    )
    if binary is None or not shutil.which("exiftool"):
        return ""
    try:
        et_proc = subprocess.run(  # nosec B603
            ["exiftool", "-j", "-G", str(sample_path)],
            capture_output=True, text=True, timeout=30,
        )
        ox_proc = subprocess.run(  # nosec B603
            [str(binary), "-j", "--exiftool-compat", str(sample_path)],
            capture_output=True, text=True, timeout=30,
        )
        et_tags = json.loads(et_proc.stdout)[0] if et_proc.stdout.strip() else {}
        ox_tags = json.loads(ox_proc.stdout)[0] if ox_proc.stdout.strip() else {}
    except (OSError, subprocess.SubprocessError, ValueError, IndexError):
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
            knowledge_home=None, module_name=None, table_name=None, worker_label=None,
            recheck_baseline=None, extract_evidence_fn=None, scan_fn=None):
    """Attempt to close one format's gaps via up to max_repair_rounds
    candidates, each round feeding the previous round's outcome -- build
    error, gap count, test regression, or review rejection -- plus a
    critique_fn-generated critique back into the conversation before
    trying again. Returns a result dict whose "rounds" key is the full
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
        neighbor_precedent_block=neighbor_precedent_block,
        knowledge_home=knowledge_home, module_name=module_name,
        learning_budget_tokens=config.get("learning_budget_tokens", DEFAULT_LEARNING_BUDGET_TOKENS),
        parser_floor_tokens=config.get("parser_floor_tokens", DEFAULT_PARSER_FLOOR_TOKENS),
        lessons_tail_kb=config.get("lessons_tail_kb", DEFAULT_LESSONS_TAIL_KB),
    )}]

    rounds = []  # every non-fixed round: {"diff", "reason", "critique"} -- see run_tag_loop
    diff = None

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
        if round_index == max_repair_rounds - 1:
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

    for round_index in range(max_repair_rounds):
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
            log_fn(f"[{fmt}] {reason}, reverting")
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
            reason = f"targeted tests ({fmt.lower()}) regressed:\n{t_out[-DEFAULT_MAX_TEST_OUTPUT_CHARS:]}"
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
                # Failure detail (which assertion, panic message) is usually
                # near the end, right before the "test result: FAILED" summary
                # -- the full output can run to thousands of lines for a
                # 2000+-test workspace run, so only the tail is kept.
                tail = test_output[-DEFAULT_MAX_TEST_OUTPUT_CHARS:]
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
        if round_index == max_repair_rounds - 1:
            return {
                "format": fmt, "status": "failed",
                "reason": f"rejected by review: {review_reason}", "diff": diff, "rounds": rounds,
            }
        messages.append({
            "role": "user",
            "content": f"A reviewer rejected this fix: {review_reason}\nPlease resend a corrected diff.",
        })

    # Unreachable: the loop above always returns by its last iteration
    # (round_index == max_repair_rounds - 1 is covered by every branch).
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

# Keep in sync with parallel_tag_fix_loop.py's own copy of this default --
# each worker (whether launched directly or via the parallel wrapper)
# should only ever hold one tag at a time unless config.toml says
# otherwise. See that module for the full rationale.
DEFAULT_MAX_TAGS_PER_PROCESS = 1


def run_tag_loop(config, find_gaps_fn, fix_gap_fn, state_path,
                  git_checkout_clean_fn=None, repo_root=None, log_fn=print,
                  load_state_fn=load_tag_state, save_state_fn=save_tag_state,
                  max_rounds=None, max_fails=DEFAULT_MAX_TAG_FAILS, blacklist_full=False,
                  worker_id=None, claim_stale_seconds=DEFAULT_CLAIM_STALE_SECONDS,
                  max_distinct_tags=None,
                  refresh_worktree_fn=None, max_cluster_tags=1, landed_tags_path=None,
                  heartbeat_seconds=DEFAULT_HEARTBEAT_SECONDS, time_fn=time.time):
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
            active = [
                tg for tg in tag_gaps
                if not state.get(tg["tag_key"], {}).get("blacklisted")
                and not is_claimed_by_someone_else(state.get(tg["tag_key"], {}))
                and (max_distinct_tags is None or len(seen_tag_keys) < max_distinct_tags
                     or tg["tag_key"] in seen_tag_keys)
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
                        state.pop(key, None)
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
_VALID_MODEL_PHASES = {"explore", "patch"}


def models_for_phase(models, phase):
    """Filter a model pool to entries tagged for `phase` -- untagged
    entries (phase absent/None) are eligible for every phase. Falls back
    to the full pool when the filter would be empty, so a config with no
    phase tags behaves exactly as before this feature existed."""
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
        "max_tokens": table.get("max_tokens", 4096),
        "max_prompt_tokens": table.get("max_prompt_tokens", DEFAULT_MAX_PROMPT_TOKENS),
        "reasoning_effort": table.get("reasoning_effort", "max"),
        "max_prompt_tags": table.get("max_prompt_tags", DEFAULT_MAX_PROMPT_TAGS),
        "max_prompt_file_bytes": table.get("max_prompt_file_bytes", DEFAULT_MAX_PROMPT_FILE_BYTES),
        "stream": table.get("stream", True),
        "prompt_cache": table.get("prompt_cache", "auto"),
        "thinking": table.get("thinking", True),
        "temperature": table.get("temperature", 0),
        "timeout": table.get("timeout", 120),
        "max_request_turns": table.get("max_request_turns", DEFAULT_MAX_REQUEST_TURNS),
        "max_repair_rounds": table.get("max_repair_rounds", DEFAULT_MAX_REPAIR_ROUNDS),
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
        "governor_cooldown_seconds": table.get("governor_cooldown_seconds", DEFAULT_GOVERNOR_COOLDOWN_SECONDS),
        "governor_max_cooldown_seconds": table.get("governor_max_cooldown_seconds", DEFAULT_GOVERNOR_MAX_COOLDOWN_SECONDS),
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
        "--base-ref", default=None,
        help="Shared branch this worktree was forked from (parallel_tag_fix_loop.py's own "
             "current branch at startup) -- if given, run_tag_loop fast-forwards this worktree "
             "onto its latest commits at the start of every round, so a tag retried across many "
             "rounds doesn't keep comparing against an increasingly stale snapshot while other "
             "workers merge in fixes elsewhere. Omit for a standalone run with no shared branch "
             "to refresh against.",
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
            report_path = run_format_comparison(args.only_format, args.cache_dir, out_suffix=worker_label)
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
        applied, msg = git_apply(diff_text, repo_root)
        diff_path = diff_log_dir / f"{ts}-{worker_label}-{'applied' if applied else 'rejected'}.diff"
        diff_path.write_text(diff_text)
        with manifest_path.open("a") as f:
            f.write(f"{ts} worker={worker_label} applied={applied} file={diff_path.name} apply_msg={msg[:200]!r}\n")
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

    def make_logging_call_model(phase):
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
                        f"model={model} RETRY {msg}\n"
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
                    governor_cooldown_seconds=config["governor_cooldown_seconds"],
                    governor_max_cooldown_seconds=config["governor_max_cooldown_seconds"],
                )
            except Exception as e:
                elapsed = time.time() - t0
                with req_manifest_path.open("a") as f:
                    f.write(
                        f"{ts} phase={phase} worker={worker_label} model={model} "
                        f"prompt_chars={prompt_chars} elapsed={elapsed:.1f}s ERROR={e}\n"
                    )
                raise
            elapsed = time.time() - t0
            reply_path = req_log_dir / f"{ts}-{worker_label}-{phase}-response.txt"
            reply_path.write_text(reply)
            with req_manifest_path.open("a") as f:
                f.write(
                    f"{ts} phase={phase} worker={worker_label} model={model} "
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
        """
        ts = time.strftime("%Y-%m-%dT%H:%M:%S")
        gaps_closed = result.get("gaps_closed", "?")
        line = f"{ts} worker={worker_label} tag={tag_gap['tag_key']} gaps_closed={gaps_closed}\n"
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
    # Module/table attribution isn't wired until Phase 2, so every call
    # below passes module_name=None/table_name=None (format-name
    # fallback keys, per K3).
    knowledge_home = OXIDEX_HOME

    def real_fix_tag(tag_gap, cfg, previous_attempts=None):
        fmt = tag_gap["format"]

        def current_match():
            """One fresh comparison for this tag's format -- used both
            as the M3 pre-attempt baseline (recheck_baseline, captured
            once before fix_gap's repair rounds begin) and, called
            again, by recheck() itself post-attempt each round ("read
            the tagcmp JSON before applying the diff" per spec M3).
            out_suffix keeps this out from under every other same-format
            process's feet -- see find_gaps_fn above."""
            path = run_format_comparison(fmt, args.cache_dir, out_suffix=worker_label)
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
            neighbor_precedent_block=precedent,
            knowledge_home=knowledge_home, module_name=None,
            learning_budget_tokens=cfg.get("learning_budget_tokens", DEFAULT_LEARNING_BUDGET_TOKENS),
            parser_floor_tokens=cfg.get("parser_floor_tokens", DEFAULT_PARSER_FLOOR_TOKENS),
            lessons_tail_kb=cfg.get("lessons_tail_kb", DEFAULT_LESSONS_TAIL_KB),
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
            knowledge_home=knowledge_home, module_name=None, table_name=None,
            worker_label=worker_label,
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
    )
    print(f"stopped after {summary['rounds']} rounds")
    print(f"  fixed:   {len(summary['fixed'])} tags")
    print(f"  failed:  {len(summary['failed'])} attempts")
    print(f"  skipped: {len(summary['skipped'])} tags (already fixed elsewhere)")
    print(f"  cycles reset (blacklist exhausted): {summary['cycles_reset']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
