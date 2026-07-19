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

Usage:
    uv run scripts/model_fix_loop.py
"""
import argparse
import json
import os
import re
import subprocess
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


def call_model(messages, base_url, api_key, model, max_tokens, reasoning_effort):
    """POST a chat-completions request, return the assistant's reply text."""
    url = base_url.rstrip("/") + "/chat/completions"
    body = json.dumps({
        "model": model,
        "messages": messages,
        "temperature": 0,
        "max_tokens": max_tokens,
        "reasoning_effort": reasoning_effort,
    }).encode()
    req = urllib.request.Request(
        url, data=body, method="POST",
        headers={
            "Content-Type": "application/json",
            "Authorization": f"Bearer {api_key}",
        },
    )
    with urllib.request.urlopen(req, timeout=120) as resp:
        payload = json.loads(resp.read())
    return payload["choices"][0]["message"]["content"]


def git_apply(diff_text, repo_root):
    """Apply a unified diff to the working tree. Returns (success, message)."""
    result = subprocess.run(
        ["git", "apply", "--reject", "-"],
        input=diff_text, capture_output=True, text=True, cwd=repo_root,
    )
    if result.returncode == 0:
        return True, "applied"
    return False, result.stderr


def git_checkout_clean(repo_root):
    """Discard all uncommitted changes, including untracked files."""
    subprocess.run(["git", "checkout", "--", "."], cwd=repo_root, check=True)
    subprocess.run(["git", "clean", "-fd"], cwd=repo_root, check=True)


def git_commit(message, repo_root):
    subprocess.run(["git", "add", "-A"], cwd=repo_root, check=True)
    subprocess.run(["git", "commit", "-m", message], cwd=repo_root, check=True)


def cargo_build(repo_root):
    """Build the oxidex binary. Returns (success, stderr)."""
    result = subprocess.run(
        ["cargo", "build", "--release", "--bin", "oxidex"],
        capture_output=True, text=True, cwd=repo_root,
    )
    return result.returncode == 0, result.stderr


def cargo_test_workspace(repo_root):
    """Run the full workspace test suite. Returns True if all tests pass."""
    result = subprocess.run(
        ["cargo", "test", "--workspace"],
        capture_output=True, text=True, cwd=repo_root,
    )
    return result.returncode == 0


def build_prompt(gap, repo_root=REPO_ROOT):
    missing = "\n".join(
        f"  - {t['family']}:{t['name']} = {t['value']} (sample: {t.get('source_file') or 'n/a'})"
        for t in gap["missing_tags"]
    ) or "  (none)"
    diffs = "\n".join(
        f"  - {d['tag_key']}: exiftool=\"{d['exiftool_value']}\" oxidex=\"{d['oxidex_value']}\" (sample: {d['source_file']})"
        for d in gap["value_differences"]
    ) or "  (none)"
    file_blocks = []
    for f in gap["parser_files"]:
        try:
            file_blocks.append(f"--- {f} ---\n{(repo_root / f).read_text()}")
        except OSError:
            continue
    files = "\n\n".join(file_blocks) or "(no parser files located -- search src/ yourself)"
    return (
        f"You are fixing ExifTool tag-coverage gaps in the oxidex Rust codebase, format \"{gap['format']}\".\n\n"
        f"Missing entirely (ExifTool extracts it, oxidex doesn't):\n{missing}\n\n"
        f"Value differences (both extract it, values disagree):\n{diffs}\n\n"
        f"Likely relevant source files:\n{files}\n\n"
        "Respond with a single unified diff (in a ```diff fenced block) that fixes as many of these gaps "
        "as you can correctly verify. For value differences, only fix genuine bugs, not benign formatting "
        "differences. Do not include any explanation outside the diff."
    )


def fix_gap(gap, config, *, call_model_fn=call_model, git_apply_fn=git_apply,
            git_checkout_clean_fn=git_checkout_clean, git_commit_fn=git_commit,
            cargo_build_fn=cargo_build, cargo_test_workspace_fn=cargo_test_workspace,
            recheck_fn=None, repo_root=None):
    """Attempt to close one format's gaps via a single-shot patch, with one
    repair round-trip on build failure. Returns a result dict.

    recheck_fn(format_name) -> int must return the gap count for that
    format after the attempted fix (used to confirm real progress). If not
    provided, progress can never be confirmed and the attempt always fails
    the "gap count did not decrease" check.
    """
    repo_root = repo_root or REPO_ROOT
    messages = [{"role": "user", "content": build_prompt(gap, repo_root=repo_root)}]

    built = False
    for _attempt in range(2):  # one initial attempt + one repair round-trip
        reply = call_model_fn(
            messages, config["base_url"], config["api_key"], config["model"],
            config["max_tokens"], config["reasoning_effort"],
        )
        diff = extract_diff(reply)
        if diff is None:
            return {"format": gap["format"], "status": "failed", "reason": "no diff in model response"}

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
            break

        git_checkout_clean_fn(repo_root)
        messages.append({
            "role": "user",
            "content": f"The build failed:\n{build_err}\nPlease resend a corrected diff.",
        })

    if not built:
        return {"format": gap["format"], "status": "failed", "reason": "no working fix after repair attempt"}

    remaining = recheck_fn(gap["format"]) if recheck_fn else gap["gap_count"]
    if remaining >= gap["gap_count"]:
        git_checkout_clean_fn(repo_root)
        return {"format": gap["format"], "status": "failed", "reason": "gap count did not decrease"}

    if not cargo_test_workspace_fn(repo_root):
        git_checkout_clean_fn(repo_root)
        return {"format": gap["format"], "status": "failed", "reason": "cargo test --workspace regressed"}

    closed = gap["gap_count"] - remaining
    git_commit_fn(f"fix({gap['format'].lower()}): wire {closed} missing tags (via {config['model']})", repo_root)
    return {"format": gap["format"], "status": "fixed", "gaps_closed": closed}


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


def main(argv=None):
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
    parser.add_argument("--cache-dir", default=os.environ.get("EXIFTOOL_CACHE_DIR", "/tmp/oxidex-exiftool-cache"))
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
    }

    def find_gaps_fn():
        report_path = run_full_comparison(args.cache_dir)
        return group_gaps_by_format(load_comparison_report(report_path))

    def real_fix_gap(gap, cfg):
        def recheck(fmt):
            path = run_format_comparison(fmt, args.cache_dir)
            regrouped = group_gaps_by_format(load_comparison_report(path))
            match = next((g for g in regrouped if g["format"] == fmt), None)
            return match["gap_count"] if match else 0

        return fix_gap(gap, cfg, recheck_fn=recheck)

    summary = run_loop(config, find_gaps_fn, real_fix_gap)
    print(f"stopped after {summary['rounds']} rounds")
    print(f"  fixed:   {len(summary['fixed'])} formats")
    print(f"  failed:  {len(summary['failed'])} attempts")
    print(f"  skipped: {', '.join(summary['skipped']) or '(none)'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
