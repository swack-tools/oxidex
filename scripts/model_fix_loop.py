#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Close oxidex/ExifTool tag-coverage gaps via any OpenAI-compatible model API.

Config (env vars, or matching --flags):
    MODEL_FIX_BASE_URL             e.g. https://api.z.ai/api/paas/v4  (GLM-5.2)
    MODEL_FIX_API_KEY
    MODEL_FIX_MODEL                e.g. "glm-5.2"
    MODEL_FIX_MAX_TOKENS           default 4096
    MODEL_FIX_REASONING_EFFORT     default "max"
    MODEL_FIX_MAX_PROMPT_TAGS      default 40 (per-attempt cap on missing_tags/
                                    value_differences shown; the rest resurface
                                    in later rounds automatically)
    MODEL_FIX_MAX_PROMPT_FILE_BYTES default 60000 (per-attempt cap on total
                                    parser-file source bytes included)
    MODEL_FIX_STREAM                default false ("true"/"1"/"yes"/"on" to
                                    enable; requests the response as
                                    OpenAI-compatible SSE and reassembles it
                                    into the same full-string reply either way)
    MODEL_FIX_THINKING              default true; "false"/"0"/"no"/"off" sends
                                    "thinking": {"type": "disabled"} in the
                                    request body. True omits the field entirely
                                    (the API's own default), rather than
                                    guessing at an "enabled" shape the docs
                                    don't show.
    MODEL_FIX_TEMPERATURE           default 0 (deterministic)

    REVIEW_BASE_URL, REVIEW_API_KEY, REVIEW_MODEL, REVIEW_MAX_TOKENS,
    REVIEW_REASONING_EFFORT, REVIEW_STREAM, REVIEW_THINKING, REVIEW_TEMPERATURE
    -- same meaning as the MODEL_FIX_* equivalents above, but for the outer
    loop's reviewer call instead of the fixer. Each falls back to its
    MODEL_FIX_* counterpart when unset, so the reviewer reuses the fixer's
    model/config by default -- set only the ones you want to differ (e.g.
    REVIEW_MODEL alone, to review with a different model while still fixing
    with the original one).

Usage:
    uv run scripts/model_fix_loop.py
    uv run scripts/model_fix_loop.py --only-format JPEG
"""
import argparse
import json
import os
import re
import subprocess  # nosec B404 -- list-argv only, no shell=True anywhere below
import sys
import urllib.request

from find_tag_gaps import (
    REPO_ROOT,
    group_gaps_by_format,
    load_comparison_report,
    run_format_comparison,
    run_full_comparison,
)

DIFF_BLOCK_RE = re.compile(r"```diff[ \t]*\r?\n(.*?)```", re.DOTALL)


def extract_diff(response_text):
    """Pull a unified diff out of a chat response.

    Prefers a fenced ```diff block; falls back to treating the whole
    response as a diff if it looks like one (starts with "diff --git" or
    "--- "). Returns None if nothing diff-shaped is found.
    """
    match = DIFF_BLOCK_RE.search(response_text)
    if match:
        return match.group(1).strip() + "\n"
    stripped = response_text.strip()
    if stripped.startswith("diff --git") or stripped.startswith("--- "):
        return stripped + "\n"
    return None


def call_model(messages, base_url, api_key, model, max_tokens, reasoning_effort, stream=False, thinking=True,
                temperature=0):
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
        },
    )
    # base_url is developer-supplied local config (MODEL_FIX_BASE_URL /
    # REVIEW_BASE_URL), never network- or attacker-controlled input.
    with urllib.request.urlopen(req, timeout=120) as resp:  # nosec B310
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
    """
    result = subprocess.run(  # nosec B603
        ["git", "apply", "--reject", "-"],
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
    """Build the oxidex binary. Returns (success, stderr)."""
    result = subprocess.run(  # nosec B603
        ["cargo", "build", "--release", "--bin", "oxidex"],
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


def build_prompt(gap, repo_root=REPO_ROOT, max_tags=DEFAULT_MAX_PROMPT_TAGS,
                  max_file_bytes=DEFAULT_MAX_PROMPT_FILE_BYTES):
    """Format one gap into a model prompt, capped so a huge format (e.g.
    JPEG with thousands of gaps and dozens of parser files) becomes an
    iterative, tractable request instead of one impossibly large prompt.
    Whatever's omitted here resurfaces in a later round automatically,
    since gap["gap_count"] (used by fix_gap's verification) always
    reflects the format's real total, not just what's shown below.
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

    return (
        f"You are fixing ExifTool tag-coverage gaps in the oxidex Rust codebase, format \"{gap['format']}\".\n\n"
        f"Missing entirely (ExifTool extracts it, oxidex doesn't):\n{missing}\n\n"
        f"Value differences (both extract it, values disagree):\n{diffs}\n\n"
        f"Likely relevant source files:\n{files}\n\n"
        "Respond with a single unified diff (in a ```diff fenced block) that fixes as many of these gaps "
        "as you can correctly verify. For value differences, only fix genuine bugs, not benign formatting "
        "differences. Do not include any explanation outside the diff. If more gaps exist than are shown "
        "above, that's expected -- just fix what's shown here, and future rounds will address the rest."
    )


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


def review_verdict(gap, diff, config, call_model_fn=call_model):
    """Ask the model to review a diff for genuineness (not gaming the
    sample file). Uses the same call_model_fn/config as the fixer --
    reviewing is just another chat-completions call, same provider."""
    prompt = build_review_prompt(gap, diff)
    try:
        reply = call_model_fn(
            [{"role": "user", "content": prompt}],
            config["base_url"], config["api_key"], config["model"],
            config["max_tokens"], config["reasoning_effort"],
            config.get("stream", False), config.get("thinking", True),
            config.get("temperature", 0),
        )
    except Exception as e:
        return False, f"review call failed: {e}"
    return extract_review_verdict(reply)


def attempt_build(messages, *, call_model_fn, git_apply_fn, git_checkout_clean_fn,
                   cargo_build_fn, config, repo_root):
    """Try to get a working build via up to 2 model calls (initial + one
    apply/build repair round-trip), extending the given messages
    conversation in place. Returns (built, reason, diff, messages) --
    reason is None when built is True; diff is the successfully-applied
    diff (None if not built).
    """
    for _attempt in range(2):  # one initial attempt + one repair round-trip
        try:
            reply = call_model_fn(
                messages, config["base_url"], config["api_key"], config["model"],
                config["max_tokens"], config["reasoning_effort"],
                config.get("stream", False), config.get("thinking", True),
                config.get("temperature", 0),
            )
        except Exception as e:
            # Network/timeout/HTTP/malformed-response failures are a normal
            # cost of "any model" -- a single bad call must not kill the
            # whole loop. No repair round-trip here: retrying the same
            # oversized/slow request immediately is unlikely to help; the
            # cross-round 2-strikes skip-list is what handles this format
            # long-term if it keeps failing.
            return False, f"model call failed: {e}", None, messages

        diff = extract_diff(reply)
        if diff is None:
            return False, "no diff in model response", None, messages

        messages.append({"role": "assistant", "content": reply})

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
            review_config=None, recheck_fn=None, repo_root=None):
    """Attempt to close one format's gaps via a single-shot patch. Up to
    two candidates: the initial fix, and one repair round-trip if a
    reviewer rejects the first. Returns a result dict.

    review_config, if provided, is the config dict used for the review
    call instead of the fixer's own config -- lets the outer loop's
    reviewer run on a different model/provider than the fixer. Defaults
    to reusing config, matching the original single-config behavior.

    recheck_fn(format_name) -> int must return the gap count for that
    format after the attempted fix (used to confirm real progress). If not
    provided, progress can never be confirmed and the attempt always fails
    the "gap count did not decrease" check.
    """
    repo_root = repo_root or REPO_ROOT
    review_config = review_config or config
    messages = [{"role": "user", "content": build_prompt(
        gap, repo_root=repo_root,
        max_tags=config["max_prompt_tags"],
        max_file_bytes=config["max_prompt_file_bytes"],
    )}]

    review_reason = None
    for _review_attempt in range(2):  # initial candidate + one review-driven repair
        built, reason, diff, messages = attempt_build_fn(
            messages,
            call_model_fn=call_model_fn, git_apply_fn=git_apply_fn,
            git_checkout_clean_fn=git_checkout_clean_fn, cargo_build_fn=cargo_build_fn,
            config=config, repo_root=repo_root,
        )
        if not built:
            return {"format": gap["format"], "status": "failed", "reason": reason}

        remaining = recheck_fn(gap["format"]) if recheck_fn else gap["gap_count"]
        if remaining >= gap["gap_count"]:
            git_checkout_clean_fn(repo_root)
            return {"format": gap["format"], "status": "failed", "reason": "gap count did not decrease"}

        if not cargo_test_workspace_fn(repo_root):
            git_checkout_clean_fn(repo_root)
            return {"format": gap["format"], "status": "failed", "reason": "cargo test --workspace regressed"}

        approved, review_reason = review_fn(gap, diff, review_config, call_model_fn=call_model_fn)
        if approved:
            closed = gap["gap_count"] - remaining
            git_commit_fn(
                f"fix({gap['format'].lower()}): wire {closed} missing tags (via {config['model']})",
                repo_root,
            )
            return {"format": gap["format"], "status": "fixed", "gaps_closed": closed}

        git_checkout_clean_fn(repo_root)
        messages.append({
            "role": "user",
            "content": f"A reviewer rejected this fix: {review_reason}\nPlease resend a corrected diff.",
        })

    return {
        "format": gap["format"], "status": "failed",
        "reason": f"rejected by review after repair attempt: {review_reason}",
    }


def run_loop(config, find_gaps_fn, fix_gap_fn, max_dry_rounds=2):
    """Loop-until-dry driver. Returns a summary dict.

    A round is dry iff it closes zero gaps (not "discovers nothing new").
    A format that fails twice across rounds is skipped for the rest of
    the run.
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

        dry_rounds = 0 if closed_this_round else dry_rounds + 1

    return {
        "rounds": round_num,
        "fixed": fixed,
        "failed": failed,
        "skipped": sorted(set(skipped)),
    }


def _load_dotenv(path):
    """Minimal .env loader (KEY=VALUE per line, # comments and blank lines
    skipped) -- stdlib only, no python-dotenv dependency. A real
    environment variable always wins over the file, matching standard
    dotenv semantics. Missing file is a silent no-op.
    """
    if not path.is_file():
        return
    for line in path.read_text().splitlines():
        line = line.strip()
        if not line or line.startswith("#") or "=" not in line:
            continue
        key, _, value = line.partition("=")
        key = key.strip()
        if key and key not in os.environ:
            os.environ[key] = value.strip()


def main(argv=None):
    _load_dotenv(REPO_ROOT / ".env")
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base-url", default=os.environ.get("MODEL_FIX_BASE_URL"))
    parser.add_argument("--api-key", default=os.environ.get("MODEL_FIX_API_KEY"))
    parser.add_argument("--model", default=os.environ.get("MODEL_FIX_MODEL"))
    parser.add_argument(
        "--max-tokens", type=int,
        default=int(os.environ.get("MODEL_FIX_MAX_TOKENS", "4096")),
    )
    parser.add_argument(
        "--reasoning-effort",
        default=os.environ.get("MODEL_FIX_REASONING_EFFORT", "max"),
    )
    parser.add_argument(
        "--max-prompt-tags", type=int,
        default=int(os.environ.get("MODEL_FIX_MAX_PROMPT_TAGS", str(DEFAULT_MAX_PROMPT_TAGS))),
    )
    parser.add_argument(
        "--max-prompt-file-bytes", type=int,
        default=int(os.environ.get("MODEL_FIX_MAX_PROMPT_FILE_BYTES", str(DEFAULT_MAX_PROMPT_FILE_BYTES))),
    )
    parser.add_argument(
        "--stream",
        type=lambda v: str(v).strip().lower() in ("1", "true", "yes", "on"),
        default=os.environ.get("MODEL_FIX_STREAM", "false").strip().lower() in ("1", "true", "yes", "on"),
    )
    parser.add_argument(
        "--thinking",
        type=lambda v: str(v).strip().lower() in ("1", "true", "yes", "on"),
        default=os.environ.get("MODEL_FIX_THINKING", "true").strip().lower() in ("1", "true", "yes", "on"),
    )
    parser.add_argument(
        "--temperature", type=float,
        default=float(os.environ.get("MODEL_FIX_TEMPERATURE", "0")),
    )
    # REVIEW_* config for the outer loop's reviewer model -- each falls
    # back to the corresponding MODEL_FIX_* value when unset, so setting
    # nothing here keeps today's behavior (reviewer reuses the fixer's
    # own model/config) exactly as before.
    parser.add_argument(
        "--review-base-url",
        default=os.environ.get("REVIEW_BASE_URL", os.environ.get("MODEL_FIX_BASE_URL")),
    )
    parser.add_argument(
        "--review-api-key",
        default=os.environ.get("REVIEW_API_KEY", os.environ.get("MODEL_FIX_API_KEY")),
    )
    parser.add_argument(
        "--review-model",
        default=os.environ.get("REVIEW_MODEL", os.environ.get("MODEL_FIX_MODEL")),
    )
    parser.add_argument(
        "--review-max-tokens", type=int,
        default=int(os.environ.get("REVIEW_MAX_TOKENS", os.environ.get("MODEL_FIX_MAX_TOKENS", "4096"))),
    )
    parser.add_argument(
        "--review-reasoning-effort",
        default=os.environ.get("REVIEW_REASONING_EFFORT", os.environ.get("MODEL_FIX_REASONING_EFFORT", "max")),
    )
    parser.add_argument(
        "--review-stream",
        type=lambda v: str(v).strip().lower() in ("1", "true", "yes", "on"),
        default=os.environ.get(
            "REVIEW_STREAM", os.environ.get("MODEL_FIX_STREAM", "false"),
        ).strip().lower() in ("1", "true", "yes", "on"),
    )
    parser.add_argument(
        "--review-thinking",
        type=lambda v: str(v).strip().lower() in ("1", "true", "yes", "on"),
        default=os.environ.get(
            "REVIEW_THINKING", os.environ.get("MODEL_FIX_THINKING", "true"),
        ).strip().lower() in ("1", "true", "yes", "on"),
    )
    parser.add_argument(
        "--review-temperature", type=float,
        default=float(os.environ.get("REVIEW_TEMPERATURE", os.environ.get("MODEL_FIX_TEMPERATURE", "0"))),
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
    args = parser.parse_args(argv)

    if not (args.base_url and args.api_key and args.model):
        print(
            "MODEL_FIX_BASE_URL, MODEL_FIX_API_KEY, and MODEL_FIX_MODEL "
            "(or --base-url/--api-key/--model) are all required",
            file=sys.stderr,
        )
        return 1

    config = {
        "base_url": args.base_url,
        "api_key": args.api_key,
        "model": args.model,
        "max_tokens": args.max_tokens,
        "reasoning_effort": args.reasoning_effort,
        "max_prompt_tags": args.max_prompt_tags,
        "max_prompt_file_bytes": args.max_prompt_file_bytes,
        "stream": args.stream,
        "thinking": args.thinking,
        "temperature": args.temperature,
    }

    review_config = {
        "base_url": args.review_base_url,
        "api_key": args.review_api_key,
        "model": args.review_model,
        "max_tokens": args.review_max_tokens,
        "reasoning_effort": args.review_reasoning_effort,
        "stream": args.review_stream,
        "thinking": args.review_thinking,
        "temperature": args.review_temperature,
    }

    def find_gaps_fn():
        if args.only_format:
            report_path = run_format_comparison(args.only_format, args.cache_dir)
        else:
            report_path = run_full_comparison(args.cache_dir)
        gaps = group_gaps_by_format(load_comparison_report(report_path))
        if args.only_format:
            gaps = [g for g in gaps if g["format"] == args.only_format]
        return gaps

    def real_fix_gap(gap, cfg):
        def recheck(fmt):
            path = run_format_comparison(fmt, args.cache_dir)
            regrouped = group_gaps_by_format(load_comparison_report(path))
            match = next((g for g in regrouped if g["format"] == fmt), None)
            return match["gap_count"] if match else 0

        return fix_gap(gap, cfg, recheck_fn=recheck, review_config=review_config)

    summary = run_loop(config, find_gaps_fn, real_fix_gap)
    print(f"stopped after {summary['rounds']} rounds")
    print(f"  fixed:   {len(summary['fixed'])} formats")
    print(f"  failed:  {len(summary['failed'])} attempts")
    print(f"  skipped: {', '.join(summary['skipped']) or '(none)'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
