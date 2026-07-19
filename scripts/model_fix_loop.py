#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.9"
# dependencies = []
# ///
"""Close oxidex/ExifTool tag-coverage gaps via any OpenAI-compatible model API.

Config (env vars, or matching --flags):
    MODEL_FIX_BASE_URL   e.g. https://api.z.ai/api/paas/v4  (GLM-5.2)
    MODEL_FIX_API_KEY
    MODEL_FIX_MODEL       e.g. "glm-5.2"

Usage:
    uv run scripts/model_fix_loop.py
"""
import argparse
import json
import os
import re
import subprocess
import sys
import urllib.error
import urllib.request
from pathlib import Path

from find_tag_gaps import (
    REPO_ROOT,
    group_gaps_by_format,
    load_comparison_report,
    run_format_comparison,
    run_full_comparison,
)

DIFF_BLOCK_RE = re.compile(r"```diff\n(.*?)```", re.DOTALL)


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


def call_model(messages, base_url, api_key, model):
    """POST a chat-completions request, return the assistant's reply text."""
    url = base_url.rstrip("/") + "/chat/completions"
    body = json.dumps({"model": model, "messages": messages, "temperature": 0}).encode()
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
