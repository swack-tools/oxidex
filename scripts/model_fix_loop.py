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
                          pinning to one model.
    max_tokens           default 4096
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

[reviewer] defaults to [worker] entirely when omitted, so a single table
covers both the fixer and the reviewer by default -- add [reviewer] only to
run review on a different model pool/provider.

An optional [parallel] table configures scripts/parallel_tag_fix_loop.py:

    workers                default 4 -- number of concurrent worker
                          processes, each in its own persistent worktree
    max_tags_per_process   default none (unbounded) -- stop a worker after
                          it has attempted this many distinct tags, rather
                          than running until the whole shared tag pool is
                          blacklisted/fixed

Usage:
    uv run scripts/model_fix_loop.py
    uv run scripts/model_fix_loop.py --only-format JPEG
    uv run scripts/model_fix_loop.py --config /path/to/config.toml
"""
import argparse
import json
import os
import random
import re
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import time
import tomllib
import urllib.request
from pathlib import Path

from find_tag_gaps import (
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


def call_model(messages, base_url, api_key, model, max_tokens, reasoning_effort, stream=False, thinking=True,
                temperature=0, timeout=120):
    """POST a chat-completions request, return the assistant's reply text.

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
    """
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
        capture_output=True, text=True, cwd=repo_root,
    )
    return result.returncode == 0, result.stderr


def cargo_test_workspace(repo_root):
    """Run the full workspace test suite. Returns True if all tests pass."""
    result = subprocess.run(  # nosec B603
        ["cargo", "test", "--workspace"],
        capture_output=True, text=True, cwd=repo_root,
    )
    return result.returncode == 0


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
        blocks.append(f"Attempt {i}:\n{diff_block}\nFailed because: {attempt.get('reason', 'unknown')}")
    return (
        "\n\nPrevious attempts on this exact tag, in order (learn from these -- do not "
        "repeat the same broken approach):\n\n" + "\n\n".join(blocks)
    )


def build_prompt(gap, repo_root=REPO_ROOT, max_tags=DEFAULT_MAX_PROMPT_TAGS,
                  max_file_bytes=DEFAULT_MAX_PROMPT_FILE_BYTES, samples_dir=None,
                  max_samples_listed=DEFAULT_MAX_SAMPLE_FILES_LISTED, previous_attempts=None):
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
                f"\n\nReal sample files available for this format (relative to the samples dir):\n{listed}\n\n"
                "If you need to see actual raw bytes to understand the binary layout instead of "
                "guessing from the tag values above, respond with EXACTLY one line "
                "\"REQUEST: <path>\" (a path from the list above, or a source file under src/) "
                "instead of a diff, and you'll get a hex dump (for samples) or the file's text back "
                "in the next turn to work from."
            )

    attempts_block = format_previous_attempts(previous_attempts)

    return (
        f"You are fixing ExifTool tag-coverage gaps in the oxidex Rust codebase, format \"{gap['format']}\".\n\n"
        f"Missing entirely (ExifTool extracts it, oxidex doesn't):\n{missing}\n\n"
        f"Value differences (both extract it, values disagree):\n{diffs}\n\n"
        f"Likely relevant source files:\n{files}"
        f"{samples_block}"
        f"{attempts_block}\n\n"
        "Respond with a single unified diff (in a ```diff fenced block) that fixes as many of these gaps "
        "as you can correctly verify. For value differences, only fix genuine bugs, not benign formatting "
        "differences. Do not include any explanation outside the diff. If more gaps exist than are shown "
        "above, that's expected -- just fix what's shown here, and future rounds will address the rest."
    )


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
            config["max_tokens"], config["reasoning_effort"],
            config.get("stream", False), config.get("thinking", True),
            config.get("temperature", 0), config.get("timeout", 120),
        )
    except Exception as e:
        return False, f"review call failed: {e}"
    return extract_review_verdict(reply)


REQUEST_RE = re.compile(r"^REQUEST:\s*(.+)$", re.IGNORECASE)
DEFAULT_MAX_REQUEST_TURNS = 20  # investigation turns before a diff is still required
DEFAULT_HEXDUMP_BYTES = 2048


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


def resolve_request(path_str, repo_root, samples_dir, max_text_bytes=20_000):
    """Answer a model's "REQUEST: <path>" turn -- a hex dump if the path
    resolves under samples_dir (real binary sample data), the raw text if
    it resolves under repo_root (more source to read), or a rejection
    message otherwise. Path traversal outside both roots is refused.
    """
    candidates = []
    if samples_dir is not None:
        candidates.append((Path(samples_dir) / path_str.strip(), "sample"))
    candidates.append((repo_root / path_str.strip(), "source"))

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
                f"Hex dump of {path_str} ({len(data)} bytes total, "
                f"showing first {min(len(data), DEFAULT_HEXDUMP_BYTES)}):\n"
                f"{hex_dump(data)}"
            )
        content = resolved.read_text(errors="replace")[:max_text_bytes]
        return f"Contents of {path_str}:\n{content}"

    return f"Could not resolve {path_str!r} under the samples dir or repo root -- try a path from the list shown."


def attempt_build(messages, *, call_model_fn, git_apply_fn, git_checkout_clean_fn,
                   cargo_build_fn, config, repo_root, pick_model_fn=random.choice,
                   samples_dir=None):
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
    """
    max_request_turns = config.get("max_request_turns", DEFAULT_MAX_REQUEST_TURNS)
    request_turns_used = 0
    diff_attempts_used = 0
    nudged_to_stop_investigating = False
    while diff_attempts_used < 2:  # one initial attempt + one repair round-trip
        model_spec = pick_model_fn(config["models"])
        try:
            reply = call_model_fn(
                messages, model_spec["base_url"], model_spec["api_key"], model_spec["name"],
                config["max_tokens"], config["reasoning_effort"],
                config.get("stream", False), config.get("thinking", True),
                config.get("temperature", 0), config.get("timeout", 120),
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
            if request_turns_used < max_request_turns:
                request_turns_used += 1
                answer = resolve_request(request_match.group(1), repo_root, samples_dir)
                messages.append({"role": "user", "content": answer})
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
                continue
            return False, "no diff in model response (exhausted request budget)", None, messages

        diff = extract_diff(reply)
        if diff is None:
            return False, "no diff in model response", None, messages

        diff_attempts_used += 1
        applied, apply_msg = git_apply_fn(diff, repo_root)
        if not applied:
            git_checkout_clean_fn(repo_root)
            messages.append({
                "role": "user",
                "content": f"That diff did not apply: {apply_msg}\nPlease resend a corrected diff.",
            })
            continue

        built, build_err = cargo_build_fn(repo_root)
        if built:
            return True, None, diff, messages

        git_checkout_clean_fn(repo_root)
        messages.append({
            "role": "user",
            "content": f"The build failed:\n{build_err}\nPlease resend a corrected diff.",
        })

    return False, "no working fix after repair attempt", None, messages


def fix_gap(gap, config, *, call_model_fn=call_model, git_apply_fn=git_apply,
            git_checkout_clean_fn=git_checkout_clean, git_commit_fn=git_commit,
            cargo_build_fn=cargo_build, cargo_test_workspace_fn=cargo_test_workspace,
            attempt_build_fn=attempt_build, review_fn=review_verdict,
            pick_model_fn=random.choice, log_fn=print,
            review_config=None, recheck_fn=None, repo_root=None, samples_dir=None,
            previous_attempts=None):
    """Attempt to close one format's gaps via a single-shot patch. Up to
    two candidates: the initial fix, and one repair round-trip if a
    reviewer rejects the first. Returns a result dict.

    review_config, if provided, is the config dict used for the review
    call instead of the fixer's own config -- lets the outer loop's
    reviewer run on a different model/provider than the fixer. Defaults
    to reusing config, matching the original single-config behavior.

    pick_model_fn is threaded into both attempt_build_fn and review_fn, so
    a single injected fake can make an entire fix_gap call deterministic
    in tests despite config["models"] holding multiple entries.

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
    """
    repo_root = repo_root or REPO_ROOT
    review_config = review_config or config
    fmt = gap["format"]
    messages = [{"role": "user", "content": build_prompt(
        gap, repo_root=repo_root,
        max_tags=config["max_prompt_tags"],
        max_file_bytes=config["max_prompt_file_bytes"],
        samples_dir=samples_dir,
        previous_attempts=previous_attempts,
    )}]

    review_reason = None
    for _review_attempt in range(2):  # initial candidate + one review-driven repair
        built, reason, diff, messages = attempt_build_fn(
            messages,
            call_model_fn=call_model_fn, git_apply_fn=git_apply_fn,
            git_checkout_clean_fn=git_checkout_clean_fn, cargo_build_fn=cargo_build_fn,
            config=config, repo_root=repo_root, pick_model_fn=pick_model_fn,
            samples_dir=samples_dir,
        )
        if not built:
            log_fn(f"[{fmt}] build failed: {reason}")
            return {"format": fmt, "status": "failed", "reason": reason, "diff": diff}

        remaining = recheck_fn(fmt) if recheck_fn else gap["gap_count"]
        log_fn(f"[{fmt}] gaps {gap['gap_count']} -> {remaining}")
        if remaining >= gap["gap_count"]:
            git_checkout_clean_fn(repo_root)
            log_fn(f"[{fmt}] gap count did not decrease, reverting")
            return {"format": fmt, "status": "failed", "reason": "gap count did not decrease", "diff": diff}

        if not cargo_test_workspace_fn(repo_root):
            git_checkout_clean_fn(repo_root)
            log_fn(f"[{fmt}] cargo test --workspace regressed, reverting")
            return {"format": fmt, "status": "failed", "reason": "cargo test --workspace regressed", "diff": diff}

        approved, review_reason = review_fn(
            gap, diff, review_config, call_model_fn=call_model_fn, pick_model_fn=pick_model_fn,
        )
        if approved:
            closed = gap["gap_count"] - remaining
            git_commit_fn(
                f"fix({fmt.lower()}): wire {closed} missing tags "
                f"(via {'/'.join(m['name'] for m in config['models'])})",
                repo_root,
            )
            log_fn(f"[{fmt}] FIXED: closed {closed} gaps (committed)")
            return {"format": fmt, "status": "fixed", "gaps_closed": closed}

        log_fn(f"[{fmt}] review REJECTED: {review_reason}")
        git_checkout_clean_fn(repo_root)
        messages.append({
            "role": "user",
            "content": f"A reviewer rejected this fix: {review_reason}\nPlease resend a corrected diff.",
        })

    return {
        "format": gap["format"], "status": "failed",
        "reason": f"rejected by review after repair attempt: {review_reason}",
        "diff": diff,
    }


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


def run_tag_loop(config, find_gaps_fn, fix_gap_fn, state_path,
                  git_checkout_clean_fn=None, repo_root=None, log_fn=print,
                  load_state_fn=load_tag_state, save_state_fn=save_tag_state,
                  max_rounds=None, max_fails=DEFAULT_MAX_TAG_FAILS, blacklist_full=False,
                  worker_id=None, claim_stale_seconds=1800, max_distinct_tags=None):
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
    """
    state = load_state_fn(state_path)
    fixed, failed = [], []
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
        else:
            failed.append({"tag_key": tag_gap["tag_key"], **result})
            entry["fails"] = entry.get("fails", 0) + 1
            entry.setdefault("attempts", []).append({
                "round": entry["fails"], "diff": result.get("diff"), "reason": result.get("reason", "unknown"),
            })
            if entry["fails"] >= max_fails:
                entry["blacklisted"] = True
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
        "cycles_reset": cycles_reset,
        "distinct_tags_seen": len(seen_tag_keys),
    }


DEFAULT_TAG_STATE_PATH = REPO_ROOT / "logs" / "model-fix-tag-state.json"
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


_KNOWN_MODEL_SPEC_KEYS = {"name", "base_url", "api_key"}


def _normalize_model_spec(entry, default_base_url, default_api_key):
    """Turn one models[] entry into a {"name", "base_url", "api_key"} dict.

    A plain string entry (e.g. "glm5.2-fast") uses the table's own
    base_url/api_key. A table entry (TOML inline table or [[worker.models]]
    array-of-tables) may override base_url/api_key individually, so a
    single pool can mix providers -- e.g. one wafer.ai model alongside a
    Fireworks-hosted one with its own key.

    Only name/base_url/api_key are recognized on an entry -- max_tokens,
    reasoning_effort, stream, thinking, and temperature belong on the
    parent [worker]/[reviewer] table, shared across every model in the
    pool. A misplaced key there raises immediately instead of being
    silently dropped, which is exactly what happened when max_tokens got
    written under [[worker.models]] instead of [worker]: the value never
    took effect, and nothing in the run reported that.
    """
    if isinstance(entry, str):
        return {"name": entry, "base_url": default_base_url, "api_key": default_api_key}
    unknown = set(entry) - _KNOWN_MODEL_SPEC_KEYS
    if unknown:
        raise ValueError(
            f"unrecognized key(s) {sorted(unknown)} on a models[] entry ({entry.get('name', '?')!r}) -- "
            "only name/base_url/api_key belong on an individual model entry; max_tokens, "
            "reasoning_effort, stream, thinking, and temperature belong on the parent "
            "[worker]/[reviewer] table instead, shared across every model in the pool"
        )
    return {
        "name": entry["name"],
        "base_url": entry.get("base_url", default_base_url),
        "api_key": entry.get("api_key", default_api_key),
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
        "reasoning_effort": table.get("reasoning_effort", "max"),
        "max_prompt_tags": table.get("max_prompt_tags", DEFAULT_MAX_PROMPT_TAGS),
        "max_prompt_file_bytes": table.get("max_prompt_file_bytes", DEFAULT_MAX_PROMPT_FILE_BYTES),
        "stream": table.get("stream", False),
        "thinking": table.get("thinking", True),
        "temperature": table.get("temperature", 0),
        "timeout": table.get("timeout", 120),
        "max_request_turns": table.get("max_request_turns", DEFAULT_MAX_REQUEST_TURNS),
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
        "--tag-state-path", default=str(DEFAULT_TAG_STATE_PATH),
        help=f"Where the per-tag blacklist/fail-count/attempt-history state persists "
             f"(default: {DEFAULT_TAG_STATE_PATH}). Point multiple concurrent processes at "
             "the same path to coordinate them via --worker-id claims.",
    )
    parser.add_argument(
        "--prompt-log-dir", default=str(REPO_ROOT / "logs" / "tag-fix-prompts"),
        help="Directory for process-<worker-id>-prompt.log, which every round's full prompt "
             "is appended to (also printed to stdout).",
    )
    parser.add_argument(
        "--max-tags-per-process", type=int, default=None,
        help="Cap how many distinct tags this one process will start work on before stopping "
             "(a tag already started keeps getting retried as normal). Default: unbounded. "
             "See [parallel].max_tags_per_process in config.toml for the parallel-runner default.",
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
    diff_log_dir = REPO_ROOT / "logs" / "model-fix-diffs"
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
    # through this, since fix_gap threads the same call_model_fn into both
    # attempt_build and review_fn) -- request params + prompt saved before
    # the call, response (or the exact error) saved right after, so "is it
    # even talking to the model, and what did it get back" never has to be
    # guessed at from a timeout/exception message alone.
    req_log_dir = REPO_ROOT / "logs" / "model-fix-requests"
    req_log_dir.mkdir(parents=True, exist_ok=True)
    req_manifest_path = req_log_dir / "manifest.log"

    def logging_call_model(messages, base_url, api_key, model, max_tokens, reasoning_effort,
                            stream=False, thinking=True, temperature=0, timeout=120):
        ts = time.strftime("%Y-%m-%dT%H:%M:%S")
        prompt_chars = sum(len(m.get("content", "")) for m in messages)
        req_path = req_log_dir / f"{ts}-request.json"
        req_path.write_text(json.dumps({
            "model": model, "base_url": base_url, "max_tokens": max_tokens,
            "reasoning_effort": reasoning_effort, "stream": stream,
            "thinking": thinking, "temperature": temperature, "timeout": timeout,
            "prompt_chars": prompt_chars, "messages": messages,
        }, indent=2))
        t0 = time.time()
        try:
            reply = call_model(
                messages, base_url, api_key, model, max_tokens, reasoning_effort,
                stream, thinking, temperature, timeout,
            )
        except Exception as e:
            elapsed = time.time() - t0
            with req_manifest_path.open("a") as f:
                f.write(
                    f"{ts} model={model} prompt_chars={prompt_chars} "
                    f"elapsed={elapsed:.1f}s ERROR={e}\n"
                )
            raise
        elapsed = time.time() - t0
        reply_path = req_log_dir / f"{ts}-response.txt"
        reply_path.write_text(reply)
        with req_manifest_path.open("a") as f:
            f.write(
                f"{ts} model={model} prompt_chars={prompt_chars} "
                f"elapsed={elapsed:.1f}s reply_chars={len(reply)} OK\n"
            )
        return reply

    prompt_log_dir = Path(args.prompt_log_dir)
    prompt_log_dir.mkdir(parents=True, exist_ok=True)
    worker_label = args.worker_id or "1"
    prompt_log_path = prompt_log_dir / f"process-{worker_label}-prompt.log"

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
        )
        ts = time.strftime("%Y-%m-%dT%H:%M:%S")
        banner = f"\n{'=' * 20} [{ts}] worker={worker_label} tag={tag_gap['tag_key']} {'=' * 20}\n"
        print(banner + prompt_preview)
        with prompt_log_path.open("a") as f:
            f.write(banner + prompt_preview + "\n")

        return fix_gap(
            single_gap, cfg, recheck_fn=recheck, review_config=review_config,
            git_apply_fn=logging_git_apply, log_fn=timestamped_log,
            call_model_fn=logging_call_model,
            samples_dir=Path(args.cache_dir) / "combined-samples",
            previous_attempts=previous_attempts,
        )

    summary = run_tag_loop(
        config, find_gaps_fn, real_fix_tag, state_path=args.tag_state_path,
        git_checkout_clean_fn=git_checkout_clean, repo_root=REPO_ROOT,
        log_fn=timestamped_log, max_fails=args.max_tag_fails,
        blacklist_full=args.blacklist_full, worker_id=args.worker_id,
        max_distinct_tags=args.max_tags_per_process,
    )
    print(f"stopped after {summary['rounds']} rounds")
    print(f"  fixed:   {len(summary['fixed'])} tags")
    print(f"  failed:  {len(summary['failed'])} attempts")
    print(f"  cycles reset (blacklist exhausted): {summary['cycles_reset']}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
