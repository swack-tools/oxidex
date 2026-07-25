# ExifTool Coverage Loop — Driver B Review Gate Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Before a fix commits, a second model call judges the diff for genuineness (not hardcoding/gaming the sample file it was verified against) and can reject it, triggering one repair round-trip back to the fixer.

**Architecture:** `fix_gap`'s existing apply/build retry logic is extracted into its own `attempt_build` function. `fix_gap` becomes an outer loop of up to two candidates (initial fix + one review-driven repair), each delegating to `attempt_build` for its own unchanged apply/build retry behavior, with a new `review_verdict` call gating every commit.

**Tech Stack:** Python 3.9+ stdlib only (`unittest`, `re`), same conventions as the rest of this file — no new dependencies, no new env vars (the reviewer reuses the fixer's own `config`).

## Global Constraints

- No new PyPI dependencies. Test framework is `unittest`, not `pytest`.
- No new env vars — the reviewer reuses `config["base_url"/"api_key"/"model"/"max_tokens"/"reasoning_effort"]` exactly as the fixer does.
- `gap count did not decrease` and `cargo test --workspace regressed` remain immediately terminal — no retry, unchanged from current behavior. Only review-rejection gets the one repair round-trip.
- A review call that itself fails (network/timeout/exception) is treated as a rejection, not skipped and not silently approved.
- An unparseable review response is treated as a rejection — fail-safe, never silently approve something we couldn't understand.
- `fix_gap` gains exactly two new injectable keyword parameters, `attempt_build_fn=attempt_build` and `review_fn=review_verdict`, following the file's existing dependency-injection convention (every collaborator is an injectable default).
- Full design rationale lives in `docs/plans/specs/2026-07-19-exiftool-coverage-loop-driver-b-review-gate-design.md` — consult it for anything not covered by a task below.

---

## Task 1: `build_review_prompt` and `extract_review_verdict` (pure functions)

**Files:**
- Modify: `scripts/model_fix_loop.py`
- Modify: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Produces: `build_review_prompt(gap, diff) -> str`, `extract_review_verdict(response_text) -> (approved: bool, reason: str)`. Task 2 (`review_verdict`) consumes both by name.

- [ ] **Step 1: Write the failing tests**

Append to `scripts/test_model_fix_loop.py`'s import line (add `build_review_prompt` and `extract_review_verdict` alongside the existing names):

```python
from model_fix_loop import (
    build_prompt,
    build_review_prompt,
    cargo_build,
    cargo_test_workspace,
    call_model,
    extract_diff,
    extract_review_verdict,
    fix_gap,
    git_apply,
    git_checkout_clean,
    git_commit,
    run_loop,
)
```

Insert after `BuildPromptTests` (before `class FixGapHappyPathTests`):

```python
class BuildReviewPromptTests(unittest.TestCase):
    def test_includes_diff_and_tag_names(self):
        gap = make_gap(gap_count=2)  # missing: EXIF:LensModel; diff: EXIF:ISO
        prompt = build_review_prompt(gap, "--- a/x\n+++ b/x\n")
        self.assertIn("--- a/x", prompt)
        self.assertIn("EXIF:LensModel", prompt)
        self.assertIn("EXIF:ISO", prompt)
        self.assertIn("NEF", prompt)


class ExtractReviewVerdictTests(unittest.TestCase):
    def test_approve(self):
        self.assertEqual(extract_review_verdict("APPROVE"), (True, ""))

    def test_approve_case_insensitive_with_trailing_text(self):
        approved, reason = extract_review_verdict("approve\nLooks correct.")
        self.assertTrue(approved)

    def test_reject_with_reason(self):
        approved, reason = extract_review_verdict("REJECT: hardcodes the sample's literal value")
        self.assertFalse(approved)
        self.assertEqual(reason, "hardcodes the sample's literal value")

    def test_unparseable_defaults_to_rejected(self):
        approved, reason = extract_review_verdict("I'm not sure about this one.")
        self.assertFalse(approved)
        self.assertIn("unparseable review verdict", reason)
```

Note: `make_gap()` (already defined above `BuildPromptTests`) returns a gap with `missing_tags=[{"family": "EXIF", "name": "LensModel", ...}]` and `value_differences=[{"tag_key": "EXIF:ISO", ...}]` — that's where `"EXIF:LensModel"` and `"EXIF:ISO"` come from.

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd scripts && python3 -m unittest test_model_fix_loop test_find_tag_gaps -v`
Expected: `ImportError: cannot import name 'build_review_prompt'` (neither function exists yet).

- [ ] **Step 3: Implement**

Append to `scripts/model_fix_loop.py`, after `build_prompt` and before `fix_gap`:

```python
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
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd scripts && python3 -m unittest test_model_fix_loop test_find_tag_gaps -v`
Expected: all 40 tests pass (35 existing + 5 new: 1 `BuildReviewPromptTests` + 4 `ExtractReviewVerdictTests`).

- [ ] **Step 5: Commit**

```bash
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: add build_review_prompt and extract_review_verdict"
```

---

## Task 2: `review_verdict` (composed function)

**Files:**
- Modify: `scripts/model_fix_loop.py`
- Modify: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Consumes: `build_review_prompt`, `extract_review_verdict` (Task 1), `call_model` (existing).
- Produces: `review_verdict(gap, diff, config, call_model_fn=call_model) -> (bool, str)`. Task 4 (`fix_gap`) uses this as `review_fn`'s default.

- [ ] **Step 1: Write the failing tests**

Add `review_verdict` to the import line (alongside `build_review_prompt`, etc.).

Insert after `ExtractReviewVerdictTests` (before `class FixGapHappyPathTests`):

```python
class ReviewVerdictTests(unittest.TestCase):
    def test_parses_approval_from_call_model(self):
        gap = make_gap()
        approved, reason = review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k", "model": "glm-5.2", "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=lambda messages, *a: "APPROVE",
        )
        self.assertTrue(approved)

    def test_parses_rejection_from_call_model(self):
        gap = make_gap()
        approved, reason = review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k", "model": "glm-5.2", "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=lambda messages, *a: "REJECT: hardcoded value",
        )
        self.assertFalse(approved)
        self.assertEqual(reason, "hardcoded value")

    def test_treats_call_failure_as_rejection(self):
        gap = make_gap()

        def raising(messages, *a):
            raise TimeoutError("timed out")

        approved, reason = review_verdict(
            gap, "--- a/x\n+++ b/x\n",
            {"base_url": "u", "api_key": "k", "model": "glm-5.2", "max_tokens": 4096, "reasoning_effort": "max"},
            call_model_fn=raising,
        )
        self.assertFalse(approved)
        self.assertIn("review call failed", reason)
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cd scripts && python3 -m unittest test_model_fix_loop test_find_tag_gaps -v`
Expected: `ImportError: cannot import name 'review_verdict'`.

- [ ] **Step 3: Implement**

Append to `scripts/model_fix_loop.py`, immediately after `extract_review_verdict` and before `fix_gap`:

```python
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
        )
    except Exception as e:
        return False, f"review call failed: {e}"
    return extract_review_verdict(reply)
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd scripts && python3 -m unittest test_model_fix_loop test_find_tag_gaps -v`
Expected: all 43 tests pass (40 from Task 1 + 3 new `ReviewVerdictTests`).

- [ ] **Step 5: Commit**

```bash
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: add review_verdict, composing build_review_prompt + call_model_fn"
```

---

## Task 3: Extract `attempt_build` from `fix_gap`

**Files:**
- Modify: `scripts/model_fix_loop.py`
- Modify: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Produces: `attempt_build(messages, *, call_model_fn, git_apply_fn, git_checkout_clean_fn, cargo_build_fn, config, repo_root) -> (built: bool, reason: str|None, diff: str|None, messages: list)`. Task 4 makes this `fix_gap`'s `attempt_build_fn` default.
- This task is a pure refactor: `fix_gap`'s *external* behavior must be byte-for-byte identical after this task — only its internals change (delegating to the new `attempt_build` instead of looping inline). `FixGapHappyPathTests` and the two remaining `FixGapFailureTests` tests require **zero changes** as proof of this (see Step 4).

- [ ] **Step 1: Add `attempt_build`, remove the old inline loop from `fix_gap`**

Add `attempt_build` to the test file's import line.

In `scripts/model_fix_loop.py`, insert `attempt_build` immediately before `fix_gap`:

```python
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
```

Then replace `fix_gap`'s body (everything from `built = False` through the `if not built:` block) so `fix_gap` delegates to it. The full function becomes:

```python
def fix_gap(gap, config, *, call_model_fn=call_model, git_apply_fn=git_apply,
            git_checkout_clean_fn=git_checkout_clean, git_commit_fn=git_commit,
            cargo_build_fn=cargo_build, cargo_test_workspace_fn=cargo_test_workspace,
            attempt_build_fn=attempt_build, recheck_fn=None, repo_root=None):
    """Attempt to close one format's gaps via a single-shot patch, with one
    repair round-trip on build failure. Returns a result dict.

    recheck_fn(format_name) -> int must return the gap count for that
    format after the attempted fix (used to confirm real progress). If not
    provided, progress can never be confirmed and the attempt always fails
    the "gap count did not decrease" check.
    """
    repo_root = repo_root or REPO_ROOT
    messages = [{"role": "user", "content": build_prompt(
        gap, repo_root=repo_root,
        max_tags=config["max_prompt_tags"],
        max_file_bytes=config["max_prompt_file_bytes"],
    )}]

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

    closed = gap["gap_count"] - remaining
    git_commit_fn(f"fix({gap['format'].lower()}): wire {closed} missing tags (via {config['model']})", repo_root)
    return {"format": gap["format"], "status": "fixed", "gaps_closed": closed}
```

(This is an *intermediate* state, deliberately still single-attempt at the outer level -- Task 4 turns this into the 2-attempt review loop. Do not add review logic in this task.)

- [ ] **Step 2: Move and adapt the apply/build-repair and failure tests to target `attempt_build` directly**

In `scripts/test_model_fix_loop.py`:

1. **Delete** the entire `FixGapRepairRoundTripTests` class (both tests move below).
2. **Delete** these three tests from `FixGapFailureTests`: `test_fails_after_two_build_failures`, `test_fails_when_no_diff_in_response`, `test_fails_gracefully_when_model_call_raises`. `FixGapFailureTests` keeps only `test_fails_when_gap_count_does_not_decrease` and `test_fails_when_tests_regress`, **unchanged** (this is the proof that `fix_gap`'s post-build behavior didn't change).
3. **Insert**, in place of the deleted `FixGapRepairRoundTripTests` class, a shared config constant and the new `AttemptBuildTests` class:

```python
CONFIG = {
    "base_url": "u", "api_key": "k", "model": "glm-5.2",
    "max_tokens": 4096, "reasoning_effort": "max",
    "max_prompt_tags": 40, "max_prompt_file_bytes": 60_000,
}


class AttemptBuildTests(unittest.TestCase):
    def test_builds_on_first_attempt(self):
        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertIsNone(reason)
        self.assertTrue(diff.startswith("--- a/x"))

    def test_retries_once_on_build_failure_then_succeeds(self):
        build_attempts = []

        def fake_cargo_build(root):
            build_attempts.append(1)
            if len(build_attempts) == 1:
                return False, "error[E0308]: mismatched types"
            return True, ""

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=fake_cargo_build,
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertEqual(len(build_attempts), 2)

    def test_retries_once_on_apply_failure_then_succeeds(self):
        apply_attempts = []

        def fake_git_apply(diff, root):
            apply_attempts.append(1)
            if len(apply_attempts) == 1:
                return False, "patch does not apply"
            return True, "ok"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=fake_git_apply,
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertEqual(len(apply_attempts), 2)

    def test_fails_after_two_build_failures(self):
        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (False, "still broken"),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertEqual(reason, "no working fix after repair attempt")
        self.assertIsNone(diff)

    def test_fails_when_no_diff_in_response(self):
        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=lambda messages, *a: "I could not find a fix.",
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertEqual(reason, "no diff in model response")

    def test_fails_gracefully_when_model_call_raises(self):
        def raising_call_model(messages, *a):
            raise TimeoutError("The read operation timed out")

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=raising_call_model,
            git_apply_fn=lambda diff, root: self.fail("should not apply"),
            cargo_build_fn=lambda root: self.fail("should not build"),
            git_checkout_clean_fn=lambda root: None,
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertFalse(built)
        self.assertIn("model call failed", reason)
        self.assertIn("timed out", reason)
```

- [ ] **Step 3: Run the tests to verify RED (import) then GREEN**

Run: `cd scripts && python3 -m unittest test_model_fix_loop test_find_tag_gaps -v`
Before the Step 1 implementation change, this fails with `ImportError: cannot import name 'attempt_build'`. After Step 1's implementation and Step 2's test edits:
Expected: all 44 tests pass (43 from Task 2, minus 5 removed [2 `FixGapRepairRoundTripTests` + 3 from `FixGapFailureTests`], plus 6 new `AttemptBuildTests` = 43 - 5 + 6 = 44).

- [ ] **Step 4: Confirm `fix_gap`'s external behavior is unchanged**

Run just the untouched tests to prove the refactor didn't change `fix_gap`'s behavior:
```
cd scripts && python3 -m unittest test_model_fix_loop.FixGapHappyPathTests test_model_fix_loop.FixGapFailureTests -v
```
Expected: 3 tests, all pass (`FixGapHappyPathTests` has 1, `FixGapFailureTests` now has 2) -- and critically, **none of their source code changed** in this task (verify with `git diff` before committing: the only edits inside these two classes' text should be zero).

- [ ] **Step 5: Commit**

```bash
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "refactor: extract attempt_build from fix_gap's inline apply/build loop

Pure extraction -- fix_gap delegates to the new attempt_build_fn
(defaulting to the real attempt_build) instead of looping inline.
FixGapHappyPathTests and the two remaining FixGapFailureTests tests
are unchanged, proving external behavior is identical. Sets up
fix_gap to become a review-gated outer loop in the next task."
```

---

## Task 4: Wire the review gate into `fix_gap`'s outer loop

**Files:**
- Modify: `scripts/model_fix_loop.py`
- Modify: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Consumes: `attempt_build` (Task 3), `review_verdict` (Task 2).
- Produces: `fix_gap`'s new signature includes `review_fn=review_verdict`. Nothing downstream depends on this file further -- `run_loop` and `main()` are untouched (they call `fix_gap` without specifying `review_fn`, so they pick up the new default automatically; no new env vars or CLI flags needed).

- [ ] **Step 1: Replace `fix_gap`'s body with the review-gated outer loop**

In `scripts/model_fix_loop.py`, replace the entire `fix_gap` function (the intermediate version from Task 3) with:

```python
def fix_gap(gap, config, *, call_model_fn=call_model, git_apply_fn=git_apply,
            git_checkout_clean_fn=git_checkout_clean, git_commit_fn=git_commit,
            cargo_build_fn=cargo_build, cargo_test_workspace_fn=cargo_test_workspace,
            attempt_build_fn=attempt_build, review_fn=review_verdict,
            recheck_fn=None, repo_root=None):
    """Attempt to close one format's gaps via a single-shot patch. Up to
    two candidates: the initial fix, and one repair round-trip if a
    reviewer rejects the first. Returns a result dict.

    recheck_fn(format_name) -> int must return the gap count for that
    format after the attempted fix (used to confirm real progress). If not
    provided, progress can never be confirmed and the attempt always fails
    the "gap count did not decrease" check.
    """
    repo_root = repo_root or REPO_ROOT
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

        approved, review_reason = review_fn(gap, diff, config)
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
```

- [ ] **Step 2: Update `FixGapHappyPathTests` to inject an approving reviewer**

In `scripts/test_model_fix_loop.py`, `FixGapHappyPathTests.test_commits_when_build_and_tests_pass_and_gaps_shrink` currently doesn't pass `review_fn`, so it would now fall through to the real `review_verdict` default (a real network call) unless updated. Add one keyword argument to its `fix_gap(...)` call:

```python
            review_fn=lambda g, diff, config: (True, ""),
```

(Insert it anywhere among the other keyword arguments -- e.g. right after `recheck_fn=lambda fmt: 0,`.) No other line in this test changes.

- [ ] **Step 3: Write the failing tests for the new review-retry behavior**

Insert after `FixGapFailureTests` (before `class RunLoopTests`):

```python
class FixGapReviewTests(unittest.TestCase):
    def test_retries_once_when_review_rejects_then_approves(self):
        gap = make_gap(gap_count=2)
        review_calls = []
        attempt_calls = []
        commit_calls = []

        def fake_attempt_build(messages, **kwargs):
            attempt_calls.append(len(messages))
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        def fake_review(g, diff, config):
            review_calls.append(1)
            if len(review_calls) == 1:
                return False, "hardcodes the sample value"
            return True, ""

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=fake_attempt_build,
            review_fn=fake_review,
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: commit_calls.append(msg),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "fixed")
        self.assertEqual(len(review_calls), 2)
        self.assertEqual(len(attempt_calls), 2)
        self.assertGreater(attempt_calls[1], attempt_calls[0])
        self.assertEqual(len(commit_calls), 1)

    def test_fails_after_review_rejects_twice(self):
        gap = make_gap(gap_count=2)

        def fake_attempt_build(messages, **kwargs):
            messages.append({"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```\n"})
            return True, None, "--- a/x\n+++ b/x\n", messages

        result = fix_gap(
            gap, CONFIG,
            attempt_build_fn=fake_attempt_build,
            review_fn=lambda g, diff, config: (False, "hardcodes the sample value"),
            git_checkout_clean_fn=lambda root: None,
            git_commit_fn=lambda msg, root: self.fail("should not commit"),
            cargo_test_workspace_fn=lambda root: True,
            recheck_fn=lambda fmt: 0,
            repo_root=Path("/fake/repo"),
        )

        self.assertEqual(result["status"], "failed")
        self.assertIn("rejected by review after repair attempt", result["reason"])
        self.assertIn("hardcodes the sample value", result["reason"])
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cd scripts && python3 -m unittest test_model_fix_loop test_find_tag_gaps -v`
Expected: all 46 tests pass (44 from Task 3 + 2 new `FixGapReviewTests`; `FixGapHappyPathTests` count unchanged at 1, just modified).

- [ ] **Step 5: Commit**

```bash
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: gate fix_gap commits on review_verdict, with one review-driven repair"
```

---

## Task 5: Manual sanity check against real data

This validates the new prompt against the real gap data already on disk from earlier testing this session -- no automated test needed, just eyeballing a realistic review prompt before it goes out to a real model.

- [ ] **Step 1: Run the full test suite one more time**

```bash
cd scripts && python3 -m unittest test_model_fix_loop test_find_tag_gaps -v
```
Expected: all 46 tests pass, output pristine.

- [ ] **Step 2: Render a real review prompt**

```bash
cd scripts && python3 -c "
import json
from find_tag_gaps import group_gaps_by_format
from model_fix_loop import build_review_prompt
report = json.load(open('../comparison.json'))
gaps = group_gaps_by_format(report)
smallest = min((g for g in gaps if g['gap_count'] < 10), key=lambda g: g['gap_count'], default=gaps[-1])
prompt = build_review_prompt(smallest, '--- a/example.rs\n+++ b/example.rs\n@@ -1 +1 @@\n-old\n+new\n')
print(prompt)
"
```
Expected: a readable prompt naming real tags from `comparison.json`, ending in the `APPROVE` / `REJECT: <reason>` instruction. Confirm it reads sensibly -- this is the last check before it goes to a real model in production use.

- [ ] **Step 3: Commit if Step 2 required no code changes**

If Step 2's output looks correct, there's nothing to commit for this task -- Task 4's commit already covers the shipped behavior. If Step 2 reveals a real problem (e.g., garbled formatting), fix it, re-run Steps 1-2, and commit the fix with an explanatory message before considering this plan complete.
