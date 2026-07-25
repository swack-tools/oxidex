# ExifTool Coverage Loop — Driver B Review Gate Design

## Context

Driver B (`scripts/find_tag_gaps.py` + `scripts/model_fix_loop.py`, PR #27)
verifies a fix only by "did the gap count in one sample file drop, and did
`cargo test --workspace` still pass." That's evidence of *change*, not
evidence of a *genuine* implementation — a model under pressure to close a
gap could hardcode the exact literal value seen in the one sample file it
was shown, rather than actually decoding the tag, and both checks would
still pass.

This document adds a review gate: before a fix commits, a second model call
judges the diff for exactly this failure mode (hardcoding/gaming the
specific sample) and can reject it, triggering one repair round-trip back
to the fixer.

## Goals

- Gate every commit on a second model's judgment that the diff is a
  genuine implementation, not a sample-specific hack.
- Give a rejected fix one chance to be corrected in response to the
  reviewer's specific objection, mirroring the existing build-failure
  repair pattern.
- Reuse the fixer's own model/config (no new env vars) — reviewing is just
  another chat-completions call against the same provider.
- Keep the existing terminal failure paths (`gap count did not decrease`,
  `cargo test --workspace regressed`) exactly as they are today: still
  immediately terminal, no retry. Only review-rejection gets a retry.

## Non-goals

- A separately configurable reviewer model/provider — deferred; reuses
  `config["base_url"/"api_key"/"model"/"max_tokens"/"reasoning_effort"]`
  as-is.
- A toggle to disable review — always-on; this loop's whole purpose from
  here forward is "don't ship gamed fixes," so it isn't optional.
- Deterministic (non-LLM) hardcoding heuristics (e.g. flagging literal
  sample values in the diff) as a cheaper complementary check — a
  reasonable future addition, out of scope here.
- Re-reviewing a second time after the one repair round-trip — the
  reviewer gets exactly two chances to reject (initial, and after one
  fixer repair); a second rejection is terminal.

## Architecture

`fix_gap`'s current apply/build retry loop is extracted into its own
function, `attempt_build`, and `fix_gap` becomes an outer loop of up to two
"candidates" — the initial fix and one review-driven repair — each of
which delegates to `attempt_build` for its own (unchanged) apply/build
retry behavior.

### New pure functions

- `build_review_prompt(gap, diff) -> str` — includes the applied diff and a
  short list of the specific tag *names* (not full descriptions) the fix
  was supposed to address, e.g. "this was supposed to address:
  EXIF:LensModel, EXIF:ISO" — enough for the reviewer to check the diff
  against what was actually asked for, without reintroducing the original
  prompt-bloat problem `build_prompt`'s caps already solve.
- `extract_review_verdict(response_text) -> (approved: bool, reason: str)`
  — parses a response expected to start with `APPROVE` or
  `REJECT: <reason>`. Unparseable responses return `(False, ...)` —
  fail-safe, never silently approve something we couldn't understand.

### New composed function

- `review_verdict(gap, diff, config, call_model_fn=call_model) -> (bool, str)`
  — builds the prompt, calls the model, parses the verdict. Wrapped in the
  same try/except-as-failure pattern as the existing crash fix: a review
  call that itself fails to reach the network returns `(False, "review
  call failed: ...")` — a network hiccup must never silently approve a fix.

### Extracted inner loop

- `attempt_build(messages, *, call_model_fn, git_apply_fn,
  git_checkout_clean_fn, cargo_build_fn, config, repo_root) -> (built,
  reason, diff, messages)` — today's exact 2-attempt apply/build logic,
  unchanged behavior, pulled out of `fix_gap` so it's independently
  testable and injectable. `reason` is `None` when `built` is `True`;
  `diff` is the successfully-applied diff (`None` if not built).

### `fix_gap`'s new outer loop

```
for _review_attempt in range(2):  # initial candidate + one review repair
    built, reason, diff, messages = attempt_build_fn(messages, ...)
    if not built:
        return failed(reason)                    # unchanged terminal path

    remaining = recheck_fn(gap["format"])
    if remaining >= gap["gap_count"]:
        git_checkout_clean_fn(repo_root)
        return failed("gap count did not decrease")   # unchanged, no retry

    if not cargo_test_workspace_fn(repo_root):
        git_checkout_clean_fn(repo_root)
        return failed("cargo test --workspace regressed")  # unchanged, no retry

    approved, review_reason = review_fn(gap, diff, config)
    if approved:
        git_commit_fn(...)
        return fixed(gaps_closed=...)

    git_checkout_clean_fn(repo_root)
    messages.append({"role": "user",
                      "content": f"A reviewer rejected this fix: {review_reason}\n"
                                 "Please resend a corrected diff."})

return failed(f"rejected by review after repair attempt: {review_reason}")
```

`fix_gap` gains two new injectable keyword parameters:
`attempt_build_fn=attempt_build` and `review_fn=review_verdict`, following
the same dependency-injection convention as every other collaborator
(`call_model_fn`, `git_apply_fn`, etc.).

## Data flow

```
fix_gap
  │
  ├─ attempt_build_fn(messages, ...) ──► (built, reason, diff, messages)
  │      internally: call_model_fn → extract_diff → git_apply_fn →
  │                  cargo_build_fn, up to 2 attempts, same as today
  │
  ├─ recheck_fn(format) / cargo_test_workspace_fn(repo_root)   [unchanged]
  │
  └─ review_fn(gap, diff, config) ──► (approved, review_reason)
         internally: build_review_prompt → call_model_fn →
                     extract_review_verdict
```

Worst case per gap: 2 fixer-call attempts (initial candidate) + 1 review
call + 2 more fixer-call attempts (repair candidate) + 1 more review call =
up to 4 fixer calls + 2 review calls. Typical happy path: 1 fixer call + 1
review call.

## Error handling

- **Model-call failure during a build attempt** (network/timeout/HTTP
  error): unchanged from the existing crash fix — `attempt_build` catches
  it and returns `(False, "model call failed: ...", None, messages)`,
  immediately terminal for that candidate (no repair, matching today's
  behavior for a failed fixer call).
- **Model-call failure during review**: `review_verdict` catches it and
  returns `(False, "review call failed: ...")` — treated exactly like a
  normal rejection, so it consumes the one review-driven repair attempt
  rather than crashing or silently approving.
- **Unparseable review response**: treated as rejected, same reasoning —
  never trust an ambiguous signal as approval.
- **`gap count did not decrease` / `cargo test --workspace regressed`**:
  unchanged, immediately terminal, no retry — these represent the fix
  itself being objectively wrong or harmful, not a judgment call a second
  model opinion should get to argue with.

## Testing

- `BuildReviewPromptTests` — prompt includes the diff verbatim and the
  gap's tag names.
- `ExtractReviewVerdictTests` — approve case, reject-with-reason case,
  unparseable-defaults-to-rejected case.
- `ReviewVerdictTests` — mocked `call_model_fn`: verdict parsed correctly
  on success; exception → `(False, "review call failed: ...")`.
- `AttemptBuildTests` — the extracted inner loop, covering the same
  scenarios currently tested through `fix_gap` (happy path, apply-failure
  repair, build-failure repair, exhausted after 2 attempts, no-diff
  response, model-call exception) — these move from testing `fix_gap`
  indirectly to testing `attempt_build` directly.
- `FixGapTests` (rewritten around the new outer loop, with
  `attempt_build_fn` faked so these tests don't need to simulate
  apply/build at all): happy path (built immediately, review approves,
  commits); review rejects then approves on retry (verify
  `attempt_build_fn` invoked twice with a growing `messages` list
  containing the rejection feedback); review rejects twice (fails,
  `"rejected by review after repair attempt"`, no commit); `attempt_build_fn`
  fails outright (propagates its reason unchanged, single attempt, no
  review call made); `gap count did not decrease` / `cargo test --workspace
  regressed` remain single-attempt, no retry, even with review otherwise
  wired up.

No new PyPI dependencies, no new env vars, `unittest` throughout, matching
every other decision in this project.
