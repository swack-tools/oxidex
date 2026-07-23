# Fixer Harness Hardening: Perl→Rust Guardrails, Protocol Discipline, and Tiered Model Routing

**Date:** 2026-07-23
**Branch:** `feat/model-fix-loop-context` (PR #41)
**File under change:** `scripts/model_fix_loop.py` (+ `config.toml`, `config.example.toml`, tests)

## Motivation

The model-fix loop just moved from `max_tokens = 1000000` to a hard 4096-token
budget on both requests and responses (with the `PATCH i/N` chunked-diff
protocol for oversized diffs). That fix stops the rate-limit bleeding, but this
session's observed failure modes remain:

- Workers spend entire rounds in open-ended `REQUEST:` context-gathering,
  with prompt sizes climbing 46K → 107K → 185K chars round over round, never
  reaching a diff.
- When diffs do come, the recurring rejection reasons are architectural
  "Perl in Rust" mistakes (dynamic-typing crutches, wrong-scope scans, naming
  divergence) that a static constraints block could have preempted.
- Every conversation turn re-sends the entire growing history at full size —
  no compaction, no line-range reads — so the token budget gets eaten by
  redundant context instead of signal.
- One expensive top-tier model (`gpt-5.6-sol` at `reasoning_effort = "max"`)
  is used for every call, including trivial "here's the file you asked for"
  investigation turns.

This design hardens the single-worker, single-conversation harness. It does
NOT introduce multi-worker hand-off (state ledgers, specialized worker
profiles, escalation triggers) — that is a different architecture, explicitly
out of scope.

## Features

### F1 — `RUST_ARCHITECTURE_CONSTRAINTS` prompt block (bucket 1)

A static block alongside `KNOWN_PITFALLS`, included in every `build_prompt`
output. Already drafted and wired; this spec adds two bullets and freezes the
content:

1. No dynamic-typing crutches: no `Box<dyn Any>`, `serde_json::Value`, or new
   ad hoc `HashMap<String, X>` mimicking Perl autovivified hashes — use the
   codebase's existing `TagValue` enum and `MetadataMap` (`src/core/`).
2. No `regex` crate for binary/byte-level parsing — slice `&[u8]` directly
   (or nom/winnow where the surrounding file already uses it).
3. No self-referential structs for IFD/directory trees — store absolute byte
   offsets (`usize`) or indices, matching ExifTool's own offset-based
   traversal.
4. No large inlined lookup tables (MakerNote-style dictionaries) in a diff —
   wire through `oxidex-tags-*` / `lookup_tag_name()`.
5. No new global mutable state — thread context (byte order, base offset) as
   explicit parameters, matching neighboring functions.
6. No `unwrap()`/`expect()`/`panic!()` on data derived from the parsed file —
   propagate via `Result<T, ExifToolError>` (`src/error/`).
7. **(new)** Endianness travels through function signatures (an explicit
   byte-order parameter or the file's existing endian-aware reader), never
   through globals or implicit state.
8. **(new)** Perl builtin translation map, one line:
   `unpack("N", $val)` → `u32::from_be_bytes`, `unpack("V", $val)` →
   `u32::from_le_bytes`, `substr($val, off, len)` → slice indexing
   `&val[off..off+len]` with bounds checks.

### F2 — Reply-shape manifest + richer plan-before-diff (bucket 2)

`build_prompt`'s closing instructions enumerate exactly four valid reply
shapes:

1. **`REQUEST: <path>`** (bare line, nothing else) — see a source file or
   sample. Also `REQUEST: <path>:<start>-<end>` for a line range (F3).
2. **`VERIFY`** + one ```diff fenced block — trial-compile a candidate diff
   without committing to it (F5).
3. **`PATCH i/N`** + one ```diff fenced chunk — for final diffs too large for
   one reply (already implemented).
4. **Plan + diff** — 2-3 sentences (which tags, where in the code, what is
   different from prior attempts, and *what was learned from the previous
   turn's output*), then a single ```diff fenced block.

Control shapes (1-3) must be bare — no narrative before the control line —
so `REQUEST_RE`/`PATCH_HEADER_RE`/`VERIFY` detection keeps working.

Additionally, a one-sentence shadow-environment framing: *"You are operating
in an ephemeral, isolated git worktree; broken builds during investigation
are expected and cost nothing — probe aggressively with VERIFY rather than
guessing."*

The "self-critique / what-I-learned" idea from the reference material is
folded into shape 4's plan sentences rather than being a separate block, so
it never contaminates control-shape detection.

### F3 — Line-range `REQUEST:` reads (bucket 2)

`resolve_request` accepts `REQUEST: src/parsers/x.rs:40-120`:

- Applies to **source files only** (samples keep whole-file hex dumps —
  binary offsets don't map to lines).
- 1-indexed, inclusive, clamped to the file's real length; returns the lines
  prefixed with their line numbers so the model can request adjacent ranges
  precisely.
- Malformed ranges (start > end, non-numeric) fall back to whole-file with a
  note, rather than erroring the turn.
- Parsing lives in a pure helper (`parse_request_range(path_str) ->
  (path, start, end)`) for direct testing.

The prompt's REQUEST documentation advertises the syntax and encourages
ranges over whole files for anything large.

### F4 — Dead-end abort on repeated identical REQUESTs (bucket 2)

`attempt_build` tracks a per-attempt counter of normalized REQUEST strings
(exact path incl. range suffix, stripped). On the **3rd identical** request
(`max_request_repeats = 3`, configurable):

- Do not re-serve the content.
- Inject a pivot nudge: the path was already provided, re-reading it will not
  change anything; pivot to a different file, use a line range, VERIFY a
  hypothesis, or submit the best diff now.
- Subsequent identical repeats flow into the existing exhausted-request-budget
  nudge/failure machinery (no new terminal state).

### F5 — `VERIFY` incremental compile-check (bucket 2, new capability)

The model can trial-compile before committing to a final diff:

- **Shape:** first line `VERIFY`, then one ```diff fenced block.
- **Harness behavior:** apply the diff to the worktree → run
  `cargo check --workspace` (fast; no codegen, no tests) → capture combined
  output, tail-trimmed to `DEFAULT_MAX_CHECK_OUTPUT_CHARS = 3000` (same
  rationale as test-output trimming: Rust errors summarize at the end) →
  **revert** via the existing `git_checkout_clean_fn` → send the trimmed
  output back as the next user turn.
- **Budget:** `max_verify_turns = 10` (configurable) per `attempt_build`
  invocation, independent of `max_request_turns` and of the 2-diff-attempt
  budget. A VERIFY turn never consumes a diff attempt. On exhaustion the
  reply says so and demands the final diff.
- **Malformed VERIFY** (no diff block, diff fails to apply): error message
  back; still consumes a verify turn (prevents free looping).
- **Wiring:** new `cargo_check(repo_root) -> (success, output)` mirroring
  `cargo_build`; threaded as injectable `cargo_check_fn` through
  `fix_gap → attempt_build`. When `cargo_check_fn` is `None` (old callers,
  existing tests), a VERIFY reply gets "VERIFY unavailable in this run —
  submit your diff" and the loop continues; `main()` always wires the real
  function.
- `RUN_TEST`/`RUN_CLIPPY` mid-loop are **deferred**: `cargo check` catches
  the dominant failure class (compile errors), and full tests already gate
  every accepted diff in `fix_gap`.

### F6 — Context compaction (buckets 2/4)

Before each model call in `attempt_build`, if the estimated conversation size
(`estimate_tokens` over all message contents) exceeds
`compaction_trigger_tokens = 12000`:

- Replace the **contents** of older user turns that carry large served
  payloads (REQUEST answers, VERIFY outputs — identified by size:
  estimated tokens > `DEFAULT_COMPACTION_MIN_ELIDE_TOKENS = 1000`, a module
  constant, not a config knob) with a one-line stub:
  `[earlier content elided for space: <first line of the original>. Re-REQUEST it (ideally with a line range) if still needed.]`
- Never touch: the initial prompt (message 0), the last
  `compaction_keep_recent_turns = 4` messages, or any assistant message
  (the model's own diffs/PATCH chunks must survive verbatim for chunk
  reassembly and repair context).
- Idempotent — already-stubbed turns are skipped.
- Pure function `compact_messages(messages, trigger_tokens, keep_recent) ->
  messages` for direct testing; `attempt_build` calls it in place.

### F7 — Cache-friendly prompt ordering (bucket 4)

Reorder `build_prompt` output so the byte-stable prefix is maximal across
rounds and retries (provider-side prompt caching), and so truncation keeps
the essentials:

1. **Static, format-independent:** role line, `RUST_ARCHITECTURE_CONSTRAINTS`,
   `KNOWN_PITFALLS`, the full reply-shape manifest + instructions (F2),
   `ARCHITECTURE_PRIMER`.
2. **Stable per-format/tag:** gap description (missing/diffs), format
   overview + ExifTool NOTES, parser files, sample listings, Perl reference
   block.
3. **Volatile (changes every round):** sweep-review history, format memory,
   previous attempts.
4. **Terminal reminder (tiny, static):** two lines restating "reply with
   exactly one of the four shapes above" — kept at the very end because
   models weight endings, while remaining byte-stable.

`truncate_to_token_budget` continues to keep the head, which now correctly
prioritizes constraints + gap description over volatile history.

### F8 — Tiered model routing: explore vs patch (bucket 4)

Per-model `phase` and `reasoning_effort` on `[[worker.models]]` entries:

- `_KNOWN_MODEL_SPEC_KEYS` grows to
  `{name, base_url, api_key, phase, reasoning_effort}`. `phase` is
  `"explore"`, `"patch"`, or absent (= eligible for both). Any other value
  raises at config load, matching the existing strict-key behavior.
- New pure helper `models_for_phase(models, phase)` — returns entries whose
  phase matches or is absent; falls back to the full pool when the filter
  would be empty (so a single-model config keeps working untouched).
- **Deterministic phase rule inside `attempt_build`:**
  - **explore:** the initial call of a fresh attempt, and any call whose
    immediately preceding user turn is a served REQUEST answer or VERIFY
    output.
  - **patch:** any call whose preceding user turn demands a diff — the
    exhausted-budget nudge, dead-end pivot nudge, PATCH-chunk continuation
    prompts, apply-failure / build-failure repair prompts, and the first
    call of a `fix_gap` critique-retry round (its preceding turn is the
    critique asking for a corrected diff).
  - A spontaneous diff from an explore-tier model is accepted — it still
    passes `cargo build`, the full test suite, duplicate detection, and the
    sol-max reviewer before landing, so the patch tier's ownership of
    quality is preserved by the gates, not by refusing cheap diffs.
- **Per-call reasoning effort:** every call site uses
  `model_spec.get("reasoning_effort") or config["reasoning_effort"]`.
- Reviewer keeps its own `[reviewer]` table (sol @ max). Critique and
  format-memory summarization calls use the **explore** tier (small prompts,
  cheap model is fine).

### Config defaults (both `config.toml` and `config.example.toml`)

Delta view — existing keys (base_url, api_key, stream, thinking,
temperature, timeout, retry knobs, `[parallel]`) are unchanged:

```toml
[worker]
max_tokens = 4096
max_prompt_tokens = 4096
max_request_turns = 20
max_request_repeats = 3        # F4 dead-end threshold
max_verify_turns = 10          # F5 VERIFY budget
compaction_trigger_tokens = 12000   # F6
compaction_keep_recent_turns = 4    # F6
reasoning_effort = "max"       # table default; entries override below

[[worker.models]]
name = "gpt-5.6-terra"
phase = "explore"
reasoning_effort = "medium"

[[worker.models]]
name = "gpt-5.6-sol"
phase = "patch"
reasoning_effort = "max"

[reviewer]
reasoning_effort = "max"

[[reviewer.models]]
name = "gpt-5.6-sol"
```

`_normalize_model_config` gains the four new knobs with these defaults so a
config.toml missing them behaves identically.

## Explicitly cut (YAGNI)

- JSON/XML tool-command envelope (replaces working regex protocol; no gain).
- Chunk continuity checksums / `CONT_TOKEN` (redundant with all-indices-
  present reassembly check).
- `[PATCH_COMPLETE]` EOF sentinel (i/N already bounds the sequence).
- Hard-locked phase state machine / "turn 10 freeze" (our loop is not
  fixed-iteration; the soft manifest + routing rule covers it).
- Async streaming fast-path (large streaming-parser change, marginal gain).
- Mid-loop `RUN_TEST`/`RUN_CLIPPY` (deferred; `cargo check` first).
- All bucket-3 multi-worker hand-off machinery (state ledger, specialized
  worker profiles, escalation, artifact manifests) — different architecture.
- Dual-run parity fuzzing — the exiftool-vs-oxidex comparison pipeline
  already is this.

## Testing

TDD per feature; all logic in pure/injectable functions matching the file's
established hermetic style:

- F1/F2/F7: `build_prompt` content and **ordering** assertions (static prefix
  before gap content before volatile tail; manifest lists all four shapes;
  constraints block present).
- F3: `parse_request_range` unit tests (happy path, clamping, malformed
  fallback); `resolve_request` range behavior incl. samples exclusion.
- F4: repeated-REQUEST counting, 3rd-repeat pivot nudge, flow into existing
  budget machinery.
- F5: VERIFY happy path (apply→check→revert→output returned), malformed
  VERIFY, budget exhaustion, `cargo_check_fn=None` fallback, VERIFY never
  consuming diff attempts; `cargo_check` unit test mirroring
  `CargoTestWorkspaceTests`.
- F6: `compact_messages` — trigger threshold, stubbing only large served
  user turns, preserving message 0 / recent turns / assistant turns,
  idempotency.
- F8: `models_for_phase` filtering + empty-filter fallback; phase selection
  per the deterministic rule (tracking fake `pick_model_fn` calls);
  per-entry `reasoning_effort` reaching the call; config validation of the
  new keys.
- Existing 405 tests stay green throughout.

## Rollout

1. Land on `feat/model-fix-loop-context`, push to PR #41.
2. Merge into local `main` (`/Users/allen/git/oxidex`).
3. Update master `config.toml` + `config.example.toml` and copy both
   `model_fix_loop.py` and `config.toml` into all 20 live worker worktrees
   (`~/.oxidex/worktrees/parallel-fix/model-fix-*`).
4. Restart of the dispatcher picks everything up (config and code load at
   process start).
