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
    max_prompt_tokens     default 4096 (worker only; hard cap on the built
                          prompt itself -- see estimate_tokens/
                          truncate_to_token_budget -- a ~4 chars/token
                          estimate, no real tokenizer dependency. If the
                          model's own diff would exceed this in one reply,
                          the prompt tells it to split into "PATCH i/N"
                          chunks instead -- see attempt_build.)
    max_request_repeats    default 3 (worker only; identical REQUESTs before
                          a pivot nudge replaces the served content)
    max_verify_turns       default 10 (worker only; VERIFY trial-compile
                          turns per attempt -- see attempt_build)
    compaction_trigger_tokens      default 12000; conversation size (est.
                          tokens) beyond which stale served payloads are
                          stubbed -- see compact_messages
    compaction_keep_recent_turns   default 4; most-recent messages exempt
                          from compaction
    reasoning_effort      default "max"
    max_prompt_tags       default 40 (worker only; per-attempt cap on
                          missing_tags/value_differences shown -- the rest
                          resurface in later rounds automatically)
    max_prompt_file_bytes default 60000 (worker only; per-attempt cap on
                          total parser-file source bytes included)
    stream                default false; requests the response as
                          OpenAI-compatible SSE and reassembles it into the
                          same full-string reply either way
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
import functools
import json
import os
import random
import re
import shutil
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
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


DEFAULT_MAX_PROMPT_TOKENS = 4096


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


DEFAULT_COMPACTION_TRIGGER_TOKENS = 12_000
DEFAULT_COMPACTION_KEEP_RECENT_TURNS = 4
DEFAULT_COMPACTION_MIN_ELIDE_TOKENS = 1000
_COMPACTION_STUB_PREFIX = "[earlier content elided for space:"


def compact_messages(messages, trigger_tokens=DEFAULT_COMPACTION_TRIGGER_TOKENS,
                     keep_recent=DEFAULT_COMPACTION_KEEP_RECENT_TURNS):
    """Shrink a long conversation by stubbing out stale served payloads.

    Once the whole conversation's estimated tokens exceed trigger_tokens,
    older USER turns carrying large served content (REQUEST answers,
    VERIFY outputs -- anything over DEFAULT_COMPACTION_MIN_ELIDE_TOKENS)
    are replaced with a one-line stub naming what was elided and how to
    get it back. Never touched: message 0 (the initial prompt), the last
    keep_recent messages, and every assistant message (the model's own
    diffs/PATCH chunks must survive verbatim for chunk reassembly and
    repair context). Pure -- returns a new list; idempotent -- stubs are
    recognized and skipped on a second pass.
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
        if estimate_tokens(content) <= DEFAULT_COMPACTION_MIN_ELIDE_TOKENS:
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


DEFAULT_RETRYABLE_HTTP_STATUSES = {500, 502, 503, 504}
DEFAULT_MAX_RETRIES = 1000
DEFAULT_RETRY_BACKOFF_SECONDS = 2
DEFAULT_MAX_RETRY_BACKOFF_SECONDS = 120  # cap growth -- 2**1000 would otherwise be absurd


def call_model(messages, base_url, api_key, model, max_tokens, reasoning_effort, stream=False, thinking=True,
                temperature=0, timeout=120, max_retries=DEFAULT_MAX_RETRIES,
                retry_backoff_seconds=DEFAULT_RETRY_BACKOFF_SECONDS,
                max_retry_backoff_seconds=DEFAULT_MAX_RETRY_BACKOFF_SECONDS, sleep_fn=time.sleep,
                log_fn=None):
    """POST a chat-completions request, retrying on transient upstream
    failures, and return the assistant's reply text.

    Retries (with exponential backoff -- retry_backoff_seconds, *2, *4, ...,
    capped at max_retry_backoff_seconds so a large max_retries doesn't
    imply an absurd wait -- up to max_retries times) on: a 5xx HTTPError
    (500/502/503/504 -- server-side, not this request's fault, confirmed
    to occur in bursts across otherwise-unrelated concurrent workers), a
    connection-level URLError (DNS resolution failure, refused connection,
    TLS handshake failure, or a stalled read -- no HTTP response was ever
    received at all, confirmed live: a DNS outage on the caller's machine
    burned all 10 of one tag's fail-count attempts and got it blacklisted
    without the model ever actually being reachable), or a reply that
    comes back completely empty (a provider occasionally returns "200 OK"
    with zero content -- not a legitimate model answer, indistinguishable
    from a dropped/truncated response, and retrying is cheap compared to
    burning a whole fix attempt on it). A non-5xx HTTPError (4xx: bad
    request, auth, etc.) fails immediately -- retrying an actual
    client-side problem just wastes time and can mask a real config
    issue. max_retries is high (not unlimited) specifically to ride out a
    long transient outage rather than give up and blacklist a tag over
    infrastructure, not the tag itself, being the problem.

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
        try:
            reply = _call_model_once(
                messages, base_url, api_key, model, max_tokens, reasoning_effort,
                stream, thinking, temperature, timeout,
            )
        except urllib.error.HTTPError as e:
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
            last_error = e
            continue
        if not reply:
            last_error = last_error or RuntimeError("model returned an empty reply")
            continue
        return reply
    # last_error is only None if max_retries < 0 (range(max_retries + 1) never
    # iterates) -- guard against `raise None`, which would raise a confusing
    # TypeError instead of surfacing the actual misconfiguration.
    raise last_error or RuntimeError("call_model: max_retries < 0, no attempt was made")


def _call_model_once(messages, base_url, api_key, model, max_tokens, reasoning_effort, stream, thinking,
                      temperature, timeout):
    url = base_url.rstrip("/") + "/chat/completions"
    payload = {
        "model": model,
        "messages": messages,
        "temperature": temperature,
        "max_tokens": max_tokens,
        "reasoning_effort": reasoning_effort,
        "stream": stream,
    }
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
            payload = json.loads(resp.read())
            return payload["choices"][0]["message"]["content"]

        chunks = []
        for raw_line in resp:
            line = raw_line.decode("utf-8").strip()
            if not line.startswith("data:"):
                continue
            data = line[len("data:"):].strip()
            if data == "[DONE]":
                break
            event = json.loads(data)
            choices = event.get("choices") or []
            if not choices:
                continue  # e.g. the final usage-only chunk
            content = (choices[0].get("delta") or {}).get("content")
            if content:
                chunks.append(content)
        return "".join(chunks)


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


def git_commit(message, repo_root):
    subprocess.run(["git", "add", "-A"], cwd=repo_root, check=True)  # nosec B603
    subprocess.run(["git", "commit", "-m", message], cwd=repo_root, check=True)  # nosec B603


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
    so this never breaks an environment that doesn't have it.
    """
    env = dict(os.environ)
    if shutil.which("sccache"):
        env["RUSTC_WRAPPER"] = "sccache"
    return env


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
CRITICAL RUST ARCHITECTURE CONSTRAINTS (porting ExifTool's Perl to Rust -- do not write "Perl in Rust"):
- No dynamic-typing crutches. Do not introduce Box<dyn Any>, serde_json::Value, or a new ad hoc HashMap<String, X> as a stand-in for Perl's autovivified hashes -- use this codebase's own TagValue enum and MetadataMap (see src/core/), which already exist for exactly this.
- No regex crate for binary/byte-level parsing. This is a memory-mapped/byte-slice format; slice &[u8] directly (or use nom/winnow if the surrounding file already does) rather than treating binary data as UTF-8 text a regex can search.
- No self-referential structs for IFD/directory trees (a struct holding a reference to its own parent or child). Store an absolute byte offset (usize) or index instead, exactly like ExifTool's own IFD-offset-based traversal.
- Do not inline a large hardcoded lookup table (e.g. a full MakerNote-style tag dictionary) directly into a diff. If a tag needs a name/ID lookup, wire it through this codebase's existing tag database (oxidex-tags-*, lookup_tag_name()) instead of hand-writing a new static table.
- No new global mutable state (a `static mut`, or a bare `static` with interior mutability introduced just for this fix). Thread whatever context (byte order, base offset) is needed as an explicit parameter, matching how neighboring functions in the file already do it.
- No unwrap()/expect()/panic!() on data derived from the file being parsed. A real-world file can be corrupt or unexpected; propagate errors via this codebase's Result<T, ExifToolError> (see src/error/) so a single malformed tag can't crash the whole parse.
- Endianness travels through function signatures -- an explicit byte-order parameter or the file's existing endian-aware reader type -- never through globals or implicit state (ExifTool's own Perl mutates a global byte order; do not mirror that).
- Common Perl-builtin translations: unpack("N",...) -> u32::from_be_bytes, unpack("V",...) -> u32::from_le_bytes, unpack("n",...)/unpack("v",...) -> u16::from_be_bytes/u16::from_le_bytes, substr($v, off, len) -> a bounds-checked slice &v[off..off + len].
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


DEFAULT_MAX_FORMAT_MEMORY_CHARS = 4000


def format_memory_path(memory_dir, format_name):
    return Path(memory_dir) / f"{format_name}.md"


def load_format_memory(memory_dir, format_name):
    """Current rolling memory for this format -- a running, periodically
    condensed log of what previous rounds learned (see
    append_format_memory_note/summarize_format_memory). "" if none yet."""
    try:
        return format_memory_path(memory_dir, format_name).read_text()
    except OSError:
        return ""


def append_format_memory_note(memory_dir, format_name, note, now_fn=time.time):
    """Append one dated learning to this format's rolling memory file --
    called once per round (see main()'s real_fix_tag), regardless of
    outcome, so the memory captures the whole arc of work on a format
    over time, not just this session's sweep-review verdicts (that's
    load_recent_sweep_reviews' job -- a *human* reviewer's judgment on
    specific merged/rejected commits; this is the loop's own ongoing
    account of what it tried)."""
    path = format_memory_path(memory_dir, format_name)
    path.parent.mkdir(parents=True, exist_ok=True)
    ts = time.strftime("%Y-%m-%d", time.localtime(now_fn()))
    with path.open("a") as f:
        f.write(f"- [{ts}] {note}\n")


def build_format_memory_summary_prompt(format_name, memory_text):
    return (
        f"Below is a running log of what a fixer has learned across many rounds working on "
        f"ExifTool tag-coverage gaps for format \"{format_name}\". It's grown too large to keep "
        "showing in full every round. Condense it into a shorter bullet list of the most "
        "important, still-relevant lessons -- naming conventions that worked or didn't, tags "
        "that turned out tricky and why, decoder logic that was reused vs. reinvented, anything "
        "a future round should know before starting. Drop anything superseded, resolved, or no "
        "longer useful. Respond with ONLY the condensed bullet list, no preamble or "
        f"explanation.\n\n{memory_text}"
    )


def summarize_format_memory(memory_dir, format_name, config,
                             call_model_fn=call_model, pick_model_fn=random.choice,
                             max_chars=DEFAULT_MAX_FORMAT_MEMORY_CHARS):
    """If this format's memory has grown past max_chars, condense it via
    a model call and overwrite the file with the shorter version so it
    stays a manageable size indefinitely rather than growing without
    bound round after round. Returns True iff it actually summarized.

    Never raises and never destroys existing memory on failure -- a
    summarization call is best-effort background maintenance; if it
    fails for any reason (network, malformed reply), the existing
    (long but still usable) memory is left exactly as it was rather
    than being replaced with something broken or losing it entirely.
    """
    current = load_format_memory(memory_dir, format_name)
    if len(current) <= max_chars:
        return False
    try:
        model_spec = pick_model_fn(models_for_phase(config["models"], "explore"))
        prompt = build_format_memory_summary_prompt(format_name, current)
        reply = call_model_fn(
            [{"role": "user", "content": prompt}],
            model_spec["base_url"], model_spec["api_key"], model_spec["name"],
            config["max_tokens"], model_spec.get("reasoning_effort") or config["reasoning_effort"],
            config.get("stream", False), config.get("thinking", True),
            config.get("temperature", 0), config.get("timeout", 120),
            config.get("max_retries", DEFAULT_MAX_RETRIES),
            config.get("retry_backoff_seconds", DEFAULT_RETRY_BACKOFF_SECONDS),
            config.get("max_retry_backoff_seconds", DEFAULT_MAX_RETRY_BACKOFF_SECONDS),
        ).strip()
    except Exception:
        return False
    if not reply:
        return False
    format_memory_path(memory_dir, format_name).write_text(reply + "\n")
    return True


DEFAULT_MAX_SWEEP_REVIEW_ENTRIES = 6


def load_recent_sweep_reviews(log_path, format_name, max_entries=DEFAULT_MAX_SWEEP_REVIEW_ENTRIES):
    """Read scripts/log_sweep_review.py's JSONL log and return the most
    recent entries for format_name, newest first, capped at max_entries.

    Unlike format_previous_attempts (which only ever sees a build/test
    failure on the *exact same tag* being retried), this surfaces actual
    sweep-review verdicts -- both accepted and rejected -- across every
    tag in this format, so a fixer working on a *different* tag in the
    same format still benefits from what a reviewer already found there
    (a naming convention that was wrong, a duplicate that already
    existed, a formatting guess that didn't match real ExifTool output).

    Missing/corrupt log or nothing for this format: returns [] -- this is
    advisory context, never a hard dependency (matches
    load_tag_state's own "missing file = nothing recorded yet" handling).
    """
    if not log_path.exists():
        return []
    entries = []
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
                    entries.append(entry)
    except OSError:
        return []
    entries.reverse()  # file is append-order (oldest first); newest first for display
    return entries[:max_entries]


def format_sweep_review_history(entries):
    """Render load_recent_sweep_reviews' output into a prompt section."""
    if not entries:
        return ""
    lines = []
    for entry in entries:
        verdict = entry.get("verdict", "unknown").upper()
        tag = entry.get("tag", "?")
        reason = entry.get("reason", "no reason given")
        lines.append(f"  - {verdict} {tag}: {reason}")
    return (
        "\n\nRecent sweep-review outcomes for this format (a human reviewer's actual "
        "verdicts on other fixes in this format -- REJECTED means a diff that built and "
        "tested fine was still wrong; learn the specific reason, not just that it failed):\n"
        + "\n".join(lines)
    )


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


def build_prompt(gap, repo_root=REPO_ROOT, max_tags=DEFAULT_MAX_PROMPT_TAGS,
                  max_file_bytes=DEFAULT_MAX_PROMPT_FILE_BYTES, samples_dir=None,
                  max_samples_listed=DEFAULT_MAX_SAMPLE_FILES_LISTED, previous_attempts=None,
                  perl_lib_dir=None, sweep_review_log_path=None, format_memory_dir=None,
                  max_prompt_tokens=DEFAULT_MAX_PROMPT_TOKENS):
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
    sweep-review verdicts (accepted/rejected, with reasons) for this
    gap's format -- see load_recent_sweep_reviews/format_sweep_review_history.
    None (the default) omits this section, same reasoning as perl_lib_dir.

    format_memory_dir, if given, is used to include this format's rolling
    memory -- see load_format_memory/append_format_memory_note/
    summarize_format_memory. Unlike sweep_review_log_path (a human
    reviewer's verdicts on specific merged/rejected commits), this is the
    loop's own accumulated, periodically-condensed account of everything
    tried on this format so far. None omits this section.

    Sections are ordered static-first (constraints/pitfalls/manifest),
    then per-tag content, then volatile history, so the byte-stable
    prefix is maximal for provider prompt caching and survives
    head-keeping truncation.
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

    sweep_review_block = ""
    if sweep_review_log_path is not None:
        sweep_review_block = format_sweep_review_history(
            load_recent_sweep_reviews(sweep_review_log_path, gap["format"])
        )

    memory_block = ""
    if format_memory_dir is not None:
        memory_text = load_format_memory(format_memory_dir, gap["format"]).strip()
        if memory_text:
            memory_block = (
                "\n\nAccumulated notes from previous rounds working on this format "
                "(condensed periodically so this stays a manageable size -- treat this as "
                "background context, not necessarily about the exact tag(s) above):\n\n"
                + memory_text
            )

    attempts_block = format_previous_attempts(previous_attempts)

    manifest = build_reply_shape_manifest(max_prompt_tokens)
    prompt = (
        f"You are fixing ExifTool tag-coverage gaps in the oxidex Rust codebase, format \"{gap['format']}\".\n\n"
        f"{RUST_ARCHITECTURE_CONSTRAINTS}\n\n"
        f"{KNOWN_PITFALLS}\n\n"
        f"{manifest}\n\n"
        f"Missing entirely (ExifTool extracts it, oxidex doesn't):\n{missing}\n\n"
        f"Value differences (both extract it, values disagree):\n{diffs}"
        f"{overview_block}\n\n"
        f"Likely relevant source files:\n{files}"
        f"{samples_block}"
        f"{exact_sample_block}"
        f"{perl_block}"
        f"{sweep_review_block}"
        f"{memory_block}"
        f"{attempts_block}\n\n"
        "For value differences, only fix genuine bugs, not benign formatting differences. "
        "If more gaps exist than are shown above, that's expected -- fix what's shown here; "
        "future rounds will address the rest.\n\n"
        f"{TERMINAL_REMINDER}"
    )
    return truncate_to_token_budget(prompt, max_prompt_tokens)


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


def build_review_prompt(gap, diff):
    missing_names = ", ".join(
        f"{t['family']}:{t['name']}" for t in gap["missing_tags"][:10]
    ) or "(none)"
    diff_names = ", ".join(
        d["tag_key"] for d in gap["value_differences"][:10]
    ) or "(none)"
    return (
        f"You are reviewing a proposed fix for ExifTool tag-coverage gaps in the oxidex Rust codebase, "
        f"format \"{gap['format']}\". The fix was supposed to address (among possibly more): "
        f"missing tags [{missing_names}], value differences [{diff_names}].\n\n"
        f"Here is the diff that was applied and successfully built:\n\n{diff}\n\n"
        "Judge whether this is a genuine, general implementation of the missing tag parsing/serialization "
        "logic, or whether it games the specific sample file it was tested against -- for example, "
        "hardcoding a literal expected value instead of actually decoding it, special-casing a filename, "
        "or any other shortcut that would only work for the one file used to verify this fix.\n\n"
        "Respond with exactly one of:\n"
        "APPROVE\n"
        "or\n"
        "REJECT: <one-sentence reason>"
    )


def extract_review_verdict(response_text):
    """Parse a review response into (approved, reason). Unparseable
    responses are treated as rejections -- fail-safe, never silently
    approve something we couldn't understand."""
    stripped = response_text.strip()
    if stripped.upper().startswith("APPROVE"):
        return True, ""
    if stripped.upper().startswith("REJECT"):
        _, _, reason = stripped.partition(":")
        return False, reason.strip() or "rejected, no reason given"
    return False, f"unparseable review verdict: {stripped[:200]!r}"


def review_verdict(gap, diff, config, call_model_fn=call_model, pick_model_fn=random.choice):
    """Ask the model to review a diff for genuineness (not gaming the
    sample file).

    pick_model_fn(models) -> model_spec selects which of config["models"]
    to use for this call; defaults to a random pick, so a run with multiple
    reviewer models rotates across the pool one call at a time. Each spec is
    a {"name", "base_url", "api_key"} dict -- pool entries may span
    different providers, not just different model names on the same one.
    Injectable for deterministic tests.
    """
    prompt = build_review_prompt(gap, diff)
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
    return extract_review_verdict(reply)


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


def fix_gap(gap, config, *, call_model_fn=call_model, review_call_model_fn=None,
            critique_call_model_fn=None,
            git_apply_fn=git_apply,
            git_checkout_clean_fn=git_checkout_clean, git_commit_fn=git_commit,
            cargo_build_fn=cargo_build, cargo_test_workspace_fn=cargo_test_workspace,
            cargo_check_fn=cargo_check,
            attempt_build_fn=attempt_build, review_fn=review_verdict,
            critique_fn=critique_failed_attempt,
            pick_model_fn=random.choice, log_fn=print,
            review_config=None, recheck_fn=None, repo_root=None, samples_dir=None,
            previous_attempts=None, detect_duplicate_fn=detect_duplicate_tag_insertion,
            perl_lib_dir=None, sweep_review_log_path=None, format_memory_dir=None,
            max_repair_rounds=DEFAULT_MAX_REPAIR_ROUNDS):
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

    log_fn(str) is called with a one-line status update at every decision
    point (build result, gap delta, review verdict, commit) -- defaults to
    print, so `--only-format`'s stdout (which parallel_model_fix_loop.py
    redirects to a per-format log file) carries a live, parseable trail of
    what this attempt is doing. Pass a no-op to silence it (e.g. in tests).

    recheck_fn(format_name) -> int must return the gap count for that
    format after the attempted fix (used to confirm real progress). If not
    provided, progress can never be confirmed and the attempt always fails
    the "gap count did not decrease" check.

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

    format_memory_dir, if given, is passed straight through to build_prompt
    (see load_format_memory) so the prompt includes this format's rolling
    memory. fix_gap only reads it -- appending a new note after this call
    and triggering summarization when it grows too large is the caller's
    job (see main()'s real_fix_tag), since that's bookkeeping about the
    *result* of this call, not something fix_gap itself needs to do.
    """
    repo_root = repo_root or REPO_ROOT
    review_config = review_config or config
    review_call_model_fn = review_call_model_fn or call_model_fn
    critique_call_model_fn = critique_call_model_fn or call_model_fn
    fmt = gap["format"]
    messages = [{"role": "user", "content": build_prompt(
        gap, repo_root=repo_root,
        max_tags=config["max_prompt_tags"],
        max_file_bytes=config["max_prompt_file_bytes"],
        samples_dir=samples_dir,
        perl_lib_dir=perl_lib_dir,
        sweep_review_log_path=sweep_review_log_path,
        format_memory_dir=format_memory_dir,
        previous_attempts=previous_attempts,
        max_prompt_tokens=config.get("max_prompt_tokens", DEFAULT_MAX_PROMPT_TOKENS),
    )}]

    rounds = []  # every non-fixed round: {"diff", "reason", "critique"} -- see run_tag_loop
    diff = None

    def critique_and_continue(failure_kind, reason, round_index):
        """Shared tail for every non-"fixed"/"duplicate" outcome: get a
        critique, record the round, and either return a final failure
        dict (last round) or append a repair turn and let the caller's
        loop continue to the next round."""
        critique = critique_fn(
            gap, diff, failure_kind, reason, config,
            call_model_fn=critique_call_model_fn, pick_model_fn=pick_model_fn,
        )
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
            outcome = critique_and_continue("build_failed", reason, round_index)
            if outcome:
                return outcome
            continue

        remaining = recheck_fn(fmt) if recheck_fn else gap["gap_count"]
        log_fn(f"[{fmt}] gaps {gap['gap_count']} -> {remaining}")
        if remaining >= gap["gap_count"]:
            git_checkout_clean_fn(repo_root)
            reason = "gap count did not decrease"
            log_fn(f"[{fmt}] {reason}, reverting")
            outcome = critique_and_continue("gap_not_closed", reason, round_index)
            if outcome:
                return outcome
            continue

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
            outcome = critique_and_continue("test_regressed", reason, round_index)
            if outcome:
                return outcome
            continue

        tag_literal = tag_literal_for_gap(gap)
        if tag_literal and detect_duplicate_fn(diff, tag_literal, repo_root):
            git_checkout_clean_fn(repo_root)
            reason = f"duplicate: a handler for {tag_literal} already exists elsewhere"
            log_fn(f"[{fmt}] {reason}, reverting (not a failure -- another worker got there first)")
            return {"format": fmt, "status": "duplicate", "reason": reason, "diff": diff, "rounds": rounds}

        approved, review_reason = review_fn(
            gap, diff, review_config, call_model_fn=review_call_model_fn, pick_model_fn=pick_model_fn,
        )
        if approved:
            closed = gap["gap_count"] - remaining
            git_commit_fn(
                f"fix({fmt.lower()}): wire {closed} missing tags "
                f"(via {'/'.join(m['name'] for m in config['models'])})",
                repo_root,
            )
            log_fn(f"[{fmt}] FIXED: closed {closed} gaps (committed)")
            return {"format": fmt, "status": "fixed", "gaps_closed": closed, "rounds": rounds}

        log_fn(f"[{fmt}] review REJECTED: {review_reason}")
        git_checkout_clean_fn(repo_root)
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


def load_tag_state(path):
    """Load the persistent per-tag blacklist/fail-count state. A missing or
    corrupt file just means "nothing blacklisted yet" -- this is advisory,
    resumable state, not something worth failing a run over."""
    try:
        return json.loads(Path(path).read_text())
    except (OSError, json.JSONDecodeError):
        return {}


def save_tag_state(path, state):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(state, indent=2))


DEFAULT_MAX_TAG_FAILS = 10

# Keep in sync with parallel_tag_fix_loop.py's own copy of this default --
# each worker (whether launched directly or via the parallel wrapper)
# should only ever hold one tag at a time unless config.toml says
# otherwise. See that module for the full rationale.
DEFAULT_MAX_TAGS_PER_PROCESS = 1


def run_tag_loop(config, find_gaps_fn, fix_gap_fn, state_path,
                  git_checkout_clean_fn=None, repo_root=None, log_fn=print,
                  load_state_fn=load_tag_state, save_state_fn=save_tag_state,
                  max_rounds=None, max_fails=DEFAULT_MAX_TAG_FAILS, blacklist_full=False,
                  worker_id=None, claim_stale_seconds=1800, max_distinct_tags=None,
                  refresh_worktree_fn=None):
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

    Once every currently-known tag is either fixed or blacklisted:
      - by default (blacklist_full=False), the blacklist is cleared
        entirely and a fresh cycle starts, so a tag given up on under one
        random model pick gets a clean second chance later rather than
        being abandoned forever
      - with blacklist_full=True, the loop stops instead -- for a parallel
        run where the point IS to exhaust every tag once and report,
        rather than cycle forever

    worker_id, if given, tags this run's claim on a tag with an identity
    (see state's "claimed_by") and a timestamp, so multiple concurrent
    processes sharing the same state_path (a parallel run) don't both pick
    the same currently-unclaimed tag -- see claim_stale_seconds: a claim
    older than this is treated as abandoned (its owning process likely
    crashed) and can be re-claimed by anyone.

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
    state = load_state_fn(state_path)
    fixed, failed, skipped = [], [], []
    cycles_reset = 0
    round_num = 0
    seen_tag_keys = set()

    def is_claimed_by_someone_else(entry):
        claimed_by = entry.get("claimed_by")
        if not claimed_by or claimed_by == worker_id:
            return False
        claimed_at = entry.get("claimed_at", 0)
        return (time.time() - claimed_at) < claim_stale_seconds

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

        state = load_state_fn(state_path)  # fresh read -- other workers may have updated it
        active = [
            tg for tg in tag_gaps
            if not state.get(tg["tag_key"], {}).get("blacklisted")
            and not is_claimed_by_someone_else(state.get(tg["tag_key"], {}))
            and (max_distinct_tags is None or len(seen_tag_keys) < max_distinct_tags
                 or tg["tag_key"] in seen_tag_keys)
        ]

        if not active and max_distinct_tags is not None and len(seen_tag_keys) >= max_distinct_tags:
            log_fn(f"Reached max_distinct_tags={max_distinct_tags} for this process -- stopping.")
            break

        if not active:
            all_blacklisted = all(state.get(tg["tag_key"], {}).get("blacklisted") for tg in tag_gaps)
            if blacklist_full and all_blacklisted:
                log_fn(f"All {len(tag_gaps)} tag(s) are blacklisted -- stopping (--blacklist-full).")
                break
            if all_blacklisted:
                log_fn(
                    f"All {len(tag_gaps)} remaining tag(s) are blacklisted -- "
                    "resetting the blacklist and starting a new cycle"
                )
                state = {}
                save_state_fn(state_path, state)
                cycles_reset += 1
                continue
            # Nothing blacklisted, but everything currently claimed by
            # other (non-stale) workers -- wait rather than busy-loop.
            log_fn("All remaining tags are claimed by other workers -- waiting")
            time.sleep(5)
            continue

        tag_gap = active[0]
        seen_tag_keys.add(tag_gap["tag_key"])
        entry = state.setdefault(tag_gap["tag_key"], {"fails": 0, "blacklisted": False, "attempts": []})
        entry["claimed_by"] = worker_id
        entry["claimed_at"] = time.time()
        save_state_fn(state_path, state)

        # One line per round naming both the round number and the tag --
        # the single source watch_parallel_fix.py's dashboard reads to
        # show "what iteration is this worker on, and on what tag" without
        # having to infer it from whatever bracketed status line happens
        # to be logged deeper inside fix_gap.
        log_fn(f"round {round_num}: attempting {tag_gap['tag_key']}")

        previous_attempts = entry.get("attempts", [])
        result = fix_gap_fn(tag_gap, config, previous_attempts)

        # Re-read in case another worker touched other tags meanwhile --
        # then re-fetch this tag's own entry to mutate it in place.
        state = load_state_fn(state_path)
        entry = state.setdefault(tag_gap["tag_key"], {"fails": 0, "blacklisted": False, "attempts": []})
        entry.pop("claimed_by", None)
        entry.pop("claimed_at", None)

        if result["status"] == "fixed":
            fixed.append({"tag_key": tag_gap["tag_key"], **result})
            state.pop(tag_gap["tag_key"], None)
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
            log_fn(f"[{tag_gap['tag_key']}] SKIPPED (already fixed elsewhere)")
        else:
            failed.append({"tag_key": tag_gap["tag_key"], **result})
            entry["fails"] = entry.get("fails", 0) + 1
            # fix_gap's own "rounds" is every internal repair sub-attempt
            # (build failure, gap-not-closed, test regression, or review
            # rejection), each with its own critique -- persist all of
            # them, not just the call's final outcome, so a future round
            # sees the whole arc of what was tried and why each step
            # failed. Falls back to one flattened entry (no critique) for
            # a caller whose fix_gap_fn doesn't return "rounds".
            for sub_round in result.get("rounds") or [
                {"diff": result.get("diff"), "reason": result.get("reason", "unknown"), "critique": None}
            ]:
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
                entry["blacklisted_at"] = time.time()
                entry["blacklisted_by"] = worker_id
                log_fn(f"[{tag_gap['tag_key']}] blacklisted after {entry['fails']} failed attempts")
                if git_checkout_clean_fn and repo_root:
                    git_checkout_clean_fn(repo_root)
            else:
                log_fn(f"[{tag_gap['tag_key']}] failed attempt {entry['fails']}/{max_fails}")
            state[tag_gap["tag_key"]] = entry

        save_state_fn(state_path, state)

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
        "stream": table.get("stream", False),
        "thinking": table.get("thinking", True),
        "temperature": table.get("temperature", 0),
        "timeout": table.get("timeout", 120),
        "max_request_turns": table.get("max_request_turns", DEFAULT_MAX_REQUEST_TURNS),
        "max_repair_rounds": table.get("max_repair_rounds", DEFAULT_MAX_REPAIR_ROUNDS),
        "max_request_repeats": table.get("max_request_repeats", DEFAULT_MAX_REQUEST_REPEATS),
        "max_verify_turns": table.get("max_verify_turns", DEFAULT_MAX_VERIFY_TURNS),
        "compaction_trigger_tokens": table.get("compaction_trigger_tokens", DEFAULT_COMPACTION_TRIGGER_TOKENS),
        "compaction_keep_recent_turns": table.get("compaction_keep_recent_turns", DEFAULT_COMPACTION_KEEP_RECENT_TURNS),
        "max_retries": table.get("max_retries", DEFAULT_MAX_RETRIES),
        "retry_backoff_seconds": table.get("retry_backoff_seconds", DEFAULT_RETRY_BACKOFF_SECONDS),
        "max_retry_backoff_seconds": table.get("max_retry_backoff_seconds", DEFAULT_MAX_RETRY_BACKOFF_SECONDS),
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
    parser.add_argument(
        "--format-memory-dir", default=str(OXIDEX_HOME / "logs" / "format-memory"),
        help="Directory of <FORMAT>.md rolling-memory files (see append_format_memory_note/"
             "summarize_format_memory) -- updated after every round regardless of outcome, "
             f"condensed via a model call once it grows too large. Default: {OXIDEX_HOME / 'logs' / 'format-memory'}",
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

    def find_gaps_fn():
        if args.only_format:
            report_path = run_format_comparison(args.only_format, args.cache_dir)
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
        ts = time.strftime("%Y-%m-%dT%H:%M:%S")
        applied, msg = git_apply(diff_text, repo_root)
        diff_path = diff_log_dir / f"{ts}-{'applied' if applied else 'rejected'}.diff"
        diff_path.write_text(diff_text)
        with manifest_path.open("a") as f:
            f.write(f"{ts} applied={applied} file={diff_path.name} apply_msg={msg[:200]!r}\n")
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
    worker_label = args.worker_id or "1"

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
            req_path = req_log_dir / f"{ts}-{phase}-request.json"
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

            try:
                reply = call_model(
                    messages, base_url, api_key, model, max_tokens, reasoning_effort,
                    stream, thinking, temperature, timeout,
                    max_retries, retry_backoff_seconds, max_retry_backoff_seconds,
                    log_fn=log_retry,
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
            reply_path = req_log_dir / f"{ts}-{phase}-response.txt"
            reply_path.write_text(reply)
            with req_manifest_path.open("a") as f:
                f.write(
                    f"{ts} phase={phase} worker={worker_label} model={model} "
                    f"prompt_chars={prompt_chars} elapsed={elapsed:.1f}s reply_chars={len(reply)} OK\n"
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
    format_memory_dir = Path(args.format_memory_dir)

    def real_fix_tag(tag_gap, cfg, previous_attempts=None):
        def recheck(_fmt):
            # _fmt is ignored -- tag_gap already knows its own format;
            # fix_gap's recheck_fn(format_name) contract is reused as-is,
            # scoped here to whether this ONE tag is still present rather
            # than the whole format's gap count.
            fmt = tag_gap["format"]
            path = run_format_comparison(fmt, args.cache_dir)
            regrouped = group_gaps_by_format(load_comparison_report(path))
            match = next((g for g in regrouped if g["format"] == fmt), None)
            if not match:
                return 0
            if tag_gap["kind"] == "missing":
                fam, name = tag_gap["entry"]["family"], tag_gap["entry"]["name"]
                present = any(
                    t["family"] == fam and t["name"] == name for t in match["missing_tags"]
                )
            else:
                tk = tag_gap["entry"]["tag_key"]
                present = any(d["tag_key"] == tk for d in match["value_differences"])
            return 1 if present else 0

        single_gap = make_single_tag_gap(tag_gap)
        # Log the exact prompt this round is about to send -- to the
        # screen and to a per-worker file -- before the call goes out, so
        # "what is it sending" is visible immediately rather than only
        # reconstructable after the fact from logging_call_model's request
        # dump.
        prompt_preview = build_prompt(
            single_gap, repo_root=REPO_ROOT,
            max_tags=cfg["max_prompt_tags"], max_file_bytes=cfg["max_prompt_file_bytes"],
            samples_dir=Path(args.cache_dir) / "combined-samples",
            previous_attempts=previous_attempts,
            perl_lib_dir=perl_lib_dir,
            sweep_review_log_path=sweep_review_log_path,
            format_memory_dir=format_memory_dir,
            max_prompt_tokens=cfg.get("max_prompt_tokens", DEFAULT_MAX_PROMPT_TOKENS),
        )
        ts = time.strftime("%Y-%m-%dT%H:%M:%S")
        banner = f"\n{'=' * 20} [{ts}] worker={worker_label} tag={tag_gap['tag_key']} {'=' * 20}\n"
        print(banner + prompt_preview)
        with prompt_log_path.open("a") as f:
            f.write(banner + prompt_preview + "\n")

        result = fix_gap(
            single_gap, cfg, recheck_fn=recheck, review_config=review_config,
            git_apply_fn=logging_git_apply, log_fn=timestamped_log,
            call_model_fn=logging_call_model_fixer, review_call_model_fn=logging_call_model_reviewer,
            critique_call_model_fn=logging_call_model_critique,
            samples_dir=Path(args.cache_dir) / "combined-samples",
            previous_attempts=previous_attempts,
            perl_lib_dir=perl_lib_dir,
            sweep_review_log_path=sweep_review_log_path,
            format_memory_dir=format_memory_dir,
            max_repair_rounds=cfg.get("max_repair_rounds", DEFAULT_MAX_REPAIR_ROUNDS),
        )
        if result["status"] == "fixed":
            log_tag_found(tag_gap, result)

        # Record this round's outcome in the format's rolling memory,
        # then condense it if it's grown too large -- both regardless of
        # outcome, so the memory captures the full arc (successes and
        # failures alike), not just this one tag's own attempt history.
        fmt = tag_gap["format"]
        if result["status"] == "fixed":
            note = f"Fixed {tag_gap['tag_key']} ({result.get('gaps_closed', '?')} gap(s) closed)."
        else:
            note = f"{result['status'].upper()} {tag_gap['tag_key']}: {result.get('reason', 'unknown')}"
        append_format_memory_note(format_memory_dir, fmt, note[:500])
        summarize_format_memory(format_memory_dir, fmt, cfg, call_model_fn=logging_call_model_critique)

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
    )
    print(f"stopped after {summary['rounds']} rounds")
    print(f"  fixed:   {len(summary['fixed'])} tags")
    print(f"  failed:  {len(summary['failed'])} attempts")
    print(f"  skipped: {len(summary['skipped'])} tags (already fixed elsewhere)")
    print(f"  cycles reset (blacklist exhausted): {summary['cycles_reset']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
