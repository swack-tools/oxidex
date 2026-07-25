# ExifTool Coverage Loop — Driver B (Any-Model) Design

## Context

A companion Claude-subagent implementation of the coverage loop ("Driver A")
was designed and is being built in parallel, in a separate worktree, on
branch `claude/exiftool-coverage-loop-96b462`
(`docs/plans/specs/2026-07-19-exiftool-coverage-loop-design-driver-a.md` and
`docs/plans/2026-07-19-exiftool-coverage-loop-plan-driver-a-reference.md`,
copied into this worktree for reference only — they are not authoritative
for this branch and may drift as that work continues).

Driver A finds gaps by running `just compare-exiftool-full` (builds
`src/bin/tag-comparison`, runs it against ExifTool's own test corpus plus a
cached camera sample set) and groups the resulting `comparison.json` by
format entirely inside a Workflow script (`gapGroupsFrom()`), fixing each
format's gaps with a Claude subagent per format, isolated in its own git
worktree, merged back sequentially with a `cargo test --workspace` gate.

This document specifies **Driver B**: the same coverage-gap-closing loop,
but driven by any model reachable through an OpenAI-compatible
`/chat/completions` endpoint (GLM-5.2 via Z.ai, or any other provider) —
with no Claude Code dependency at all. It reuses the same gap signal
(`tag-comparison` / `compare-exiftool-full`) but is otherwise a fully
independent, standalone Python implementation, since Driver A's JS Workflow
logic isn't callable from outside Claude Code.

## Goals

- Close the same class of coverage gaps as Driver A (tags ExifTool extracts
  that oxidex doesn't, per format), using a model swapped in via three
  config values (base URL, API key, model name) — no code changes needed to
  point it at a different provider.
- Run standalone: no Claude Code, no Agent/Workflow tool dependency.
- Loop until two consecutive discovery rounds close zero gaps, matching
  Driver A's stop rule, then exit.
- Local commits only. No push, no PR, ever.

## Non-goals

- Parallelism. Runs strictly sequentially, one gap at a time — chosen
  specifically because the fix mechanism (single-shot patch generation,
  below) doesn't need the concurrency-safety machinery (isolated worktrees,
  a merge stage) that justified it for Driver A's multi-subagent fan-out.
  Could be added later by mirroring Driver A's worktree-per-gap approach,
  but is out of scope here.
- A full agentic tool-loop (read/write/shell tools exposed to the model) or
  shelling out to an existing coding-agent CLI (e.g. `aider`). Considered
  and rejected in favor of single-shot patch generation for simplicity: one
  prompt in, one diff out, apply mechanically.
- Write-path verification. Same caveat as Driver A — `tag-comparison` is
  read-only; this loop can only close gaps it can detect.
- Reusing Driver A's grouping logic directly — it's inline JS inside a
  Workflow script, not callable from a standalone Python process. This
  driver reimplements equivalent grouping in Python
  (`scripts/find_tag_gaps.py`), consuming the same `comparison.json` shape.

## Architecture

```
scripts/find_tag_gaps.py
  → runs `EXIFTOOL_CACHE_DIR=<cache dir> just compare-exiftool-full`
  → reads the resulting comparison.json
  → groups missing tags + value differences by format, sorts by gap count
    descending
  → emits gaps.json: [{format, missing_tags, value_differences,
    sample_files}, ...]
  → supports --only-format for targeted re-checks after a fix attempt,
    without re-running the full corpus comparison

scripts/model_fix_loop.py
  → config: MODEL_FIX_BASE_URL, MODEL_FIX_API_KEY, MODEL_FIX_MODEL
    (env vars, overridable by flags)
  → loop-until-dry over scripts/find_tag_gaps.py's output
```

### `scripts/find_tag_gaps.py`

- Wraps `just compare-exiftool-full`, same as Driver A's Find stage — same
  persisted cache directory (`EXIFTOOL_CACHE_DIR`, default
  `/tmp/oxidex-exiftool-cache`, made durable by Driver A's Task 1) so the
  two drivers can even share a warm cache if run on the same machine.
- Parses `comparison.json` (`ComparisonReport` / `FormatComparison` shape
  from `src/bin/tag-comparison/models/mod.rs`), groups `missing_in_oxidex` +
  `value_differences` by format, sorts by combined gap count descending.
- Locates each format's likely parser file(s) via the same format→directory
  heuristic already implemented in `scripts/generate_tag_coverage.py`
  (`format_map`), so the fix prompt can include real source instead of
  asking the model to search the repo blind (it has no file-search tool).
- Output: `gaps.json` by default; `--only-format NAME` re-runs
  `tag-comparison --format NAME` directly (skipping the full corpus
  comparison) for a fast single-format re-check after a fix attempt.
- Unit-testable without ExifTool or network: grouping/ordering/locator
  logic runs against a small fixture `comparison.json`.

### `scripts/model_fix_loop.py`

```
Config (env vars or flags):
  MODEL_FIX_BASE_URL   e.g. https://api.z.ai/api/paas/v4  (GLM-5.2)
  MODEL_FIX_API_KEY
  MODEL_FIX_MODEL       e.g. "glm-5.2"

loop-until-dry (stop after 2 consecutive rounds that close zero gaps):
  1. `uv run scripts/find_tag_gaps.py` → gaps.json
  2. for each gap, sequentially, biggest gap count first:
     a. prompt = missing tags + value differences + ExifTool's sample
        values + the format's located parser file contents; ask for a
        unified diff only, nothing else
     b. POST to {base_url}/chat/completions
     c. extract the diff from the response, `git apply` it
     d. `cargo build --release --bin oxidex` — on failure, one repair
        round-trip: send the compiler error back in the same chat
        (multi-turn), apply the revised diff, rebuild. Second failure →
        `git checkout -- .`, log gap failed, move on.
     e. on build success: `find_tag_gaps.py --only-format <format>` to
        confirm the gap count for that format decreased and no
        regressions appeared, then `cargo test --workspace`
     f. both clean → commit (message notes the model used, e.g.
        "fix(nef): wire N missing tags (via glm-5.2)"); anything else →
        `git checkout -- .`, log failed
  3. a gap key (format) that fails twice across rounds moves to a
     skip-list for the rest of the run, reported in the final summary
  4. loop back to step 1
```

Every touched gap's outcome (`fixed` / `failed` / `skipped`) and total
rounds run are printed as a final summary when the loop stops.

**Trust boundary note:** this sends parser source code to whatever
third-party endpoint `MODEL_FIX_BASE_URL` points at. That's inherent to
"plug in any model" and is the user's explicit choice of provider/key each
time, not something this script tries to hide.

## Data flow

```
find_tag_gaps.py --only-format <F>          (targeted re-check, reused by
    │                                         step 2e after every attempt)
    ▼
comparison.json → gaps.json (grouped, sorted)
    │
    ▼
model_fix_loop.py: for each gap group
    prompt → chat/completions → diff → git apply → cargo build
        → (retry once on build failure)
        → find_tag_gaps.py --only-format → cargo test --workspace
        → commit or revert
```

## Error handling

- **Discovery failure** (ExifTool download fails, `tag-comparison` crashes,
  `comparison.json` malformed): hard-fail the round with a clear error.
  Never treat a broken discovery step as "no gaps found."
- **Diff application failure** (model returns a non-applying diff, or no
  diff at all): counts as a build-equivalent failure — one repair
  round-trip (send the `git apply` error back to the model), then revert
  and log failed on a second miss.
- **2-strikes skip-list**: a format that fails twice across rounds is
  skipped for the rest of the run and named in the final summary, not
  silently dropped or retried forever.
- **Partial closure**: same as Driver A — a format with many gaps may not
  close in one pass; whatever remains resurfaces in the next round's
  discovery automatically.

## Testing

- `find_tag_gaps.py`: unit tests for grouping/ordering/format-locator logic
  against a small fixture `comparison.json` — no ExifTool or network
  needed.
- `model_fix_loop.py`: unit tests for the diff-apply/retry state machine
  using a stubbed HTTP response (no real API calls in CI).
- Neither script's own tests require a live model or the full ExifTool
  corpus download; only actual loop runs do.

## Open questions for the implementation plan

- Exact `gaps.json` schema (field names/types) — should mirror
  `ComparisonReport`/`FormatComparison` closely enough that a future reader
  comparing this script to Driver A's `COMPARISON_REPORT_SCHEMA` can see
  they're the same underlying data, just grouped in Python vs. JS.
- HTTP client: stdlib `urllib.request` (zero new dependencies, matching
  `jpeg_tag_matrix.py`'s `dependencies = []` convention) vs. a minimal
  `httpx`/`requests` dependency for cleaner streaming/error handling.
  Leaning stdlib unless multi-turn retry logic gets unwieldy without it.
- Diff format robustness: how strictly to parse the model's response for a
  single fenced ```diff block vs. tolerating surrounding prose.
