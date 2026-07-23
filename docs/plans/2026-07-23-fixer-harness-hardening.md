# Fixer Harness Hardening Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Harden the model-fix loop's single-conversation harness with Perl→Rust architecture guardrails, a four-shape reply protocol (REQUEST ranges, VERIFY trial-compiles, dead-end aborts, context compaction), cache-friendly prompt ordering, and tiered explore/patch model routing.

**Architecture:** All changes live in `scripts/model_fix_loop.py` (+ its test file and the two config TOMLs). Every new behavior is a pure or injectable function matching the file's established hermetic-test style. Tasks are strictly sequential — they all edit the same two files.

**Tech Stack:** Python 3.11+ stdlib only (uv inline script, `dependencies = []`), unittest, TOML config via tomllib.

**Spec:** `docs/plans/specs/2026-07-23-fixer-harness-hardening-design.md` — read it first.

## Global Constraints

- Working directory for all commands: `/Users/allen/.oxidex/worktrees/sweep-tags/scripts` (git worktree on branch `feat/model-fix-loop-context`).
- Test command: `python3 -m unittest test_model_fix_loop` (full-file). Full sweep: `python3 -m unittest discover -p "test_*.py"`. The discover suite currently passes with **405 tests**; it must pass after every task with the new tests added.
- No new dependencies — this is a uv inline script with `dependencies = []`. No tiktoken, no third-party anything.
- The working tree starts with **uncommitted changes**: the `RUST_ARCHITECTURE_CONSTRAINTS` block (6 bullets) already exists at ~line 927 of `model_fix_loop.py` and is already interpolated into `build_prompt`'s return. Task 1 finishes and commits it — do not re-create it.
- Every commit message ends with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Control-signal regexes (`REQUEST_RE`, `PATCH_HEADER_RE`, new `VERIFY_RE`) match at the START of the stripped reply. Never let new prompt text instruct the model to put narrative before a control line.
- All new config knobs are read with `config.get("<name>", DEFAULT_<NAME>)` so old configs and test CONFIG dicts keep working unchanged.

---

### Task 1: Finish `RUST_ARCHITECTURE_CONSTRAINTS` (F1) — add endianness + builtin-map bullets, tests, commit

**Files:**
- Modify: `scripts/model_fix_loop.py` (the `RUST_ARCHITECTURE_CONSTRAINTS` block, ~line 927)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Produces: module constant `RUST_ARCHITECTURE_CONSTRAINTS` (str) whose first line is `CRITICAL RUST ARCHITECTURE CONSTRAINTS (porting ExifTool's Perl to Rust -- do not write "Perl in Rust"):`. Later tasks (Task 7) interpolate it near the top of `build_prompt`.

- [ ] **Step 1: Write the failing tests**

Add to `test_model_fix_loop.py`'s import block (alphabetical position): `RUST_ARCHITECTURE_CONSTRAINTS,`. Then add this class after `BuildPromptTokenBudgetTests`:

```python
class RustArchitectureConstraintsTests(unittest.TestCase):
    def test_block_contains_the_six_core_directives(self):
        for needle in (
            "Box<dyn Any>",            # no dynamic-typing crutches
            "regex",                   # no regex on binary
            "self-referential",        # no self-referential IFD structs
            "lookup_tag_name()",       # no inlined lookup tables
            "global mutable state",    # no new globals
            "unwrap()",                # no unwrap/panic on parsed data
        ):
            self.assertIn(needle, RUST_ARCHITECTURE_CONSTRAINTS)

    def test_block_contains_endianness_and_builtin_map_bullets(self):
        self.assertIn("function signatures", RUST_ARCHITECTURE_CONSTRAINTS)
        self.assertIn("u32::from_be_bytes", RUST_ARCHITECTURE_CONSTRAINTS)
        self.assertIn("u32::from_le_bytes", RUST_ARCHITECTURE_CONSTRAINTS)

    def test_build_prompt_includes_the_constraints_block(self):
        prompt = build_prompt(make_gap(gap_count=2))
        self.assertIn("CRITICAL RUST ARCHITECTURE CONSTRAINTS", prompt)
```

- [ ] **Step 2: Run to verify the new bullets test fails**

Run: `python3 -m unittest test_model_fix_loop.RustArchitectureConstraintsTests -v`
Expected: `test_block_contains_endianness_and_builtin_map_bullets` FAILS (bullets not yet added); the other two PASS (block already exists and is wired).

- [ ] **Step 3: Append the two bullets**

In `model_fix_loop.py`, inside the `RUST_ARCHITECTURE_CONSTRAINTS` triple-quoted string, after the final existing bullet (`- No unwrap()/expect()/panic!() ...`) and before the closing `"""`, add:

```
- Endianness travels through function signatures -- an explicit byte-order parameter or the file's existing endian-aware reader type -- never through globals or implicit state (ExifTool's own Perl mutates a global byte order; do not mirror that).
- Common Perl-builtin translations: unpack("N",...) -> u32::from_be_bytes, unpack("V",...) -> u32::from_le_bytes, unpack("n",...)/unpack("v",...) -> u16::from_be_bytes/u16::from_le_bytes, substr($v, off, len) -> a bounds-checked slice &v[off..off + len].
```

- [ ] **Step 4: Run the class, then the full file**

Run: `python3 -m unittest test_model_fix_loop.RustArchitectureConstraintsTests -v` → all 3 PASS.
Run: `python3 -m unittest test_model_fix_loop` → OK (0 failures).

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: RUST_ARCHITECTURE_CONSTRAINTS prompt block (Perl->Rust guardrails)

Six non-negotiable porting directives (no dynamic-typing crutches, no
regex on binary, no self-referential IFD structs, no inlined lookup
tables, no new global state, no unwrap/panic on parsed data) plus
endianness-through-signatures and a Perl-builtin translation map,
included in every fixer prompt.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Line-range `REQUEST:` reads (F3)

**Files:**
- Modify: `scripts/model_fix_loop.py` (`resolve_request`, ~line 1492; new helper + regex just above it)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Produces: `parse_request_range(path_str) -> (path: str, start: int|None, end: int|None)` and range-aware `resolve_request` (same signature as today: `resolve_request(path_str, repo_root, samples_dir, max_text_bytes=20_000) -> str`). Task 7's manifest documents the `REQUEST: <path>:<start>-<end>` syntax; do NOT edit prompt text in this task.

- [ ] **Step 1: Write the failing tests**

Add `parse_request_range,` to the test import block. Add these classes (near the existing `ResolveRequestTests` if present, else after `RustArchitectureConstraintsTests`):

```python
class ParseRequestRangeTests(unittest.TestCase):
    def test_plain_path_has_no_range(self):
        self.assertEqual(parse_request_range("src/parsers/x.rs"), ("src/parsers/x.rs", None, None))

    def test_valid_range_is_parsed(self):
        self.assertEqual(parse_request_range("src/parsers/x.rs:40-120"), ("src/parsers/x.rs", 40, 120))

    def test_whitespace_is_stripped(self):
        self.assertEqual(parse_request_range("  src/x.rs:1-5  "), ("src/x.rs", 1, 5))

    def test_inverted_range_strips_suffix_and_falls_back_to_whole_file(self):
        self.assertEqual(parse_request_range("src/x.rs:9-3"), ("src/x.rs", None, None))

    def test_zero_start_strips_suffix_and_falls_back(self):
        self.assertEqual(parse_request_range("src/x.rs:0-5"), ("src/x.rs", None, None))

    def test_non_numeric_suffix_is_just_part_of_the_path(self):
        self.assertEqual(parse_request_range("src/x.rs:a-b"), ("src/x.rs:a-b", None, None))


class ResolveRequestRangeTests(unittest.TestCase):
    def _make_repo(self, tmpdir):
        repo = Path(tmpdir)
        (repo / "src").mkdir()
        (repo / "src" / "big.rs").write_text("\n".join(f"line{i}" for i in range(1, 101)))
        return repo

    def test_range_returns_numbered_lines(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/big.rs:5-7", repo, None)
        self.assertIn("Lines 5-7 of src/big.rs", answer)
        self.assertIn("5: line5", answer)
        self.assertIn("7: line7", answer)
        self.assertNotIn("line8", answer)

    def test_range_end_is_clamped_to_file_length(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/big.rs:98-500", repo, None)
        self.assertIn("Lines 98-100 of src/big.rs", answer)
        self.assertIn("100: line100", answer)

    def test_range_start_past_eof_returns_guidance_not_content(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            repo = self._make_repo(tmpdir)
            answer = resolve_request("src/big.rs:500-600", repo, None)
        self.assertIn("only 100 lines", answer)

    def test_sample_files_ignore_ranges_and_hex_dump_whole_file(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            samples = Path(tmpdir)
            (samples / "EXE.dylib").write_bytes(b"\xfe\xed\xfa\xcf1234")
            answer = resolve_request("EXE.dylib:1-2", Path("/nonexistent"), samples)
        self.assertIn("Hex dump of EXE.dylib", answer)
```

`resolve_request` is already imported in the test file if `ResolveRequestTests` exists; if not, add `resolve_request,` to the import block too.

- [ ] **Step 2: Run to verify failure**

Run: `python3 -m unittest test_model_fix_loop.ParseRequestRangeTests test_model_fix_loop.ResolveRequestRangeTests -v`
Expected: ImportError (`parse_request_range` not defined).

- [ ] **Step 3: Implement**

In `model_fix_loop.py`, directly above `def resolve_request(...)`:

```python
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
```

Then rework `resolve_request`. Replace its body's candidate construction and source-file branch (keep the docstring, extending it with one sentence: `A "path:START-END" suffix on a source file returns just that 1-indexed inclusive line range, numbered; samples always get the whole-file hex dump.`):

```python
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
```

(The only changes from the current body: `path_part` replaces `path_str.strip()` everywhere, and the `range_start is not None` branch is new. Hex-dump and could-not-resolve messages now use `path_part`.)

- [ ] **Step 4: Run new tests, then the full file**

Run: `python3 -m unittest test_model_fix_loop.ParseRequestRangeTests test_model_fix_loop.ResolveRequestRangeTests -v` → all PASS.
Run: `python3 -m unittest test_model_fix_loop` → OK.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: line-range REQUEST reads (REQUEST: path:START-END)

Source-file requests can name a 1-indexed inclusive line range and get
just those lines back, numbered, instead of a whole-file dump -- cuts
per-turn context bloat. Samples keep whole-file hex dumps. Malformed
ranges fall back to whole-file rather than failing the turn.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Dead-end abort on repeated identical REQUESTs (F4)

**Files:**
- Modify: `scripts/model_fix_loop.py` (`attempt_build`, ~line 1526; one new constant near `DEFAULT_MAX_REQUEST_TURNS`)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Consumes: `attempt_build`'s existing REQUEST branch (`request_match` / `request_turns_used` / `max_request_turns`).
- Produces: module constant `DEFAULT_MAX_REQUEST_REPEATS = 3`; config knob `max_request_repeats` (read via `config.get`). The pivot-nudge user message contains the phrase `Pivot:`.

- [ ] **Step 1: Write the failing tests**

Add to `AttemptBuildTests` in `test_model_fix_loop.py`:

```python
    def test_third_identical_request_gets_pivot_nudge_instead_of_content(self):
        served = []
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) <= 3:
                return "REQUEST: src/parsers/jpeg/mod.rs"
            # 4th call: after the pivot nudge, submit a diff.
            self.assertIn("Pivot:", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            (repo / "src" / "parsers" / "jpeg").mkdir(parents=True)
            (repo / "src" / "parsers" / "jpeg" / "mod.rs").write_text("real content")
            built, reason, diff, messages = attempt_build(
                [{"role": "user", "content": "fix format X"}],
                call_model_fn=fake_call_model,
                git_apply_fn=lambda diff, root: (True, "ok"),
                git_checkout_clean_fn=lambda root: None,
                cargo_build_fn=lambda root: (True, ""),
                config=CONFIG,
                repo_root=repo,
            )
        self.assertTrue(built)
        # Turns 1 and 2 served content; turn 3 got the nudge, not content.
        served_turns = [m for m in messages if m["role"] == "user" and "real content" in m["content"]]
        nudge_turns = [m for m in messages if m["role"] == "user" and "Pivot:" in m["content"]]
        self.assertEqual(len(served_turns), 2)
        self.assertEqual(len(nudge_turns), 1)

    def test_distinct_requests_do_not_trigger_the_pivot_nudge(self):
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) == 1:
                return "REQUEST: src/a.rs"
            if len(replies) == 2:
                return "REQUEST: src/b.rs"
            self.assertNotIn("Pivot:", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=CONFIG,
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)

    def test_max_request_repeats_is_configurable(self):
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) == 2:
                self.assertIn("Pivot:", messages[-1]["content"])
                return "```diff\n--- a/x\n+++ b/x\n```\n"
            return "REQUEST: src/x.rs"

        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=dict(CONFIG, max_request_repeats=1),
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
```

- [ ] **Step 2: Run to verify failure**

Run: `python3 -m unittest test_model_fix_loop.AttemptBuildTests -v 2>&1 | tail -20`
Expected: the three new tests FAIL (nudge never appears / content served three times).

- [ ] **Step 3: Implement**

Next to `DEFAULT_MAX_REQUEST_TURNS` add:

```python
DEFAULT_MAX_REQUEST_REPEATS = 3  # identical REQUESTs before a pivot nudge replaces the content
```

In `attempt_build`, add to the pre-loop initialization (next to `request_turns_used = 0`):

```python
    max_request_repeats = config.get("max_request_repeats", DEFAULT_MAX_REQUEST_REPEATS)
    request_counts = {}
```

Replace the interior of the `if request_match:` branch's within-budget path (currently `if request_turns_used < max_request_turns: request_turns_used += 1; answer = resolve_request(...); messages.append(...); continue`) with:

```python
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
                else:
                    answer = resolve_request(request_match.group(1), repo_root, samples_dir)
                    messages.append({"role": "user", "content": answer})
                continue
            # ... existing nudge / exhausted-budget code unchanged below ...
```

(Keep the existing `if not nudged_to_stop_investigating:` block and the exhausted-budget return exactly as they are — repeats past the budget flow into that machinery.)

- [ ] **Step 4: Run the class, then the full file**

Run: `python3 -m unittest test_model_fix_loop.AttemptBuildTests -v 2>&1 | tail -8` → OK.
Run: `python3 -m unittest test_model_fix_loop` → OK.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: dead-end abort -- pivot nudge on the 3rd identical REQUEST

Re-serving the same file over and over burns request budget without
advancing the attempt. The 3rd identical request (configurable via
max_request_repeats) gets a pivot nudge instead of the content; further
repeats march into the existing exhausted-budget machinery.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: `VERIFY` incremental compile-check (F5)

**Files:**
- Modify: `scripts/model_fix_loop.py` (`cargo_check` next to `cargo_build` ~line 483; constants next to `PATCH_HEADER_RE`; `attempt_build` body + signature; `fix_gap` signature + its `attempt_build_fn(...)` call)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Produces: `cargo_check(repo_root) -> (success: bool, output: str)`; `VERIFY_RE`; `DEFAULT_MAX_VERIFY_TURNS = 10`; `DEFAULT_MAX_CHECK_OUTPUT_CHARS = 3000`; `attempt_build(..., cargo_check_fn=None)`; `fix_gap(..., cargo_check_fn=cargo_check)` threading it through. Config knob `max_verify_turns`.
- Consumes: `git_apply_fn`, `git_checkout_clean_fn`, `extract_diff` — all existing.
- All existing `fix_gap` test fakes use `attempt_build_fn=lambda messages, **kwargs:` / `def fake_attempt_build(messages, **kwargs):` so the extra kwarg is absorbed — no test-fake updates needed.

- [ ] **Step 1: Write the failing tests**

Add `cargo_check,` to the test import block. Add to `test_model_fix_loop.py` after `AttemptBuildTests`:

```python
class CargoCheckTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_returns_success_and_combined_output(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="checked ok\n", stderr="warn\n")
        ok, output = cargo_check(Path("/fake/repo"))
        self.assertTrue(ok)
        self.assertEqual(output, "checked ok\nwarn\n")
        self.assertEqual(mock_run.call_args[0][0], ["cargo", "check", "--workspace"])

    @patch("model_fix_loop.subprocess.run")
    def test_nonzero_exit_is_failure(self, mock_run):
        mock_run.return_value = MagicMock(returncode=101, stdout="", stderr="error[E0308]\n")
        ok, output = cargo_check(Path("/fake/repo"))
        self.assertFalse(ok)
        self.assertIn("E0308", output)


class AttemptBuildVerifyTests(unittest.TestCase):
    def _run(self, fake_call_model, cargo_check_fn, config=None, git_apply_fn=None):
        cleans = []
        return attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=git_apply_fn or (lambda diff, root: (True, "ok")),
            git_checkout_clean_fn=lambda root: cleans.append(1),
            cargo_build_fn=lambda root: (True, ""),
            config=config or CONFIG,
            repo_root=Path("/fake/repo"),
            cargo_check_fn=cargo_check_fn,
        ), cleans

    def test_verify_applies_checks_reverts_and_reports(self):
        checks = []
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) == 1:
                return "VERIFY\n```diff\n--- a/x\n+++ b/x\n@@ -1 +1 @@\n-a\n+b\n```\n"
            self.assertIn("cargo check FAILED", messages[-1]["content"])
            self.assertIn("mismatched types", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        def fake_check(root):
            checks.append(1)
            return False, "error[E0308]: mismatched types"

        (built, reason, diff, messages), cleans = self._run(fake_call_model, fake_check)
        self.assertTrue(built)
        self.assertEqual(len(checks), 1)
        self.assertGreaterEqual(len(cleans), 1)  # trial change was reverted

    def test_verify_passing_check_reports_passed(self):
        def fake_call_model(messages, *a):
            if len(messages) == 1:
                return "VERIFY\n```diff\n--- a/x\n+++ b/x\n```\n"
            self.assertIn("cargo check PASSED", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        (built, _, _, _), _ = self._run(fake_call_model, lambda root: (True, "clean"))
        self.assertTrue(built)

    def test_verify_never_consumes_a_diff_attempt(self):
        # Two VERIFYs then two failing real diffs: the 2-diff-attempt
        # budget must still allow both real diffs.
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) <= 2:
                return "VERIFY\n```diff\n--- a/x\n+++ b/x\n```\n"
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        (built, reason, _, _), _ = self._run(
            lambda m, *a: fake_call_model(m, *a),
            lambda root: (True, "ok"),
        )
        # 3rd call is a real diff that applies and builds -> success.
        self.assertTrue(built)
        self.assertEqual(len(replies), 3)

    def test_verify_without_cargo_check_fn_gets_unavailable_message(self):
        def fake_call_model(messages, *a):
            if len(messages) == 1:
                return "VERIFY\n```diff\n--- a/x\n+++ b/x\n```\n"
            self.assertIn("VERIFY is unavailable", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        (built, _, _, _), _ = self._run(fake_call_model, None)
        self.assertTrue(built)

    def test_verify_with_no_diff_block_consumes_a_turn_and_asks_again(self):
        def fake_call_model(messages, *a):
            if len(messages) == 1:
                return "VERIFY\nI'll test changing the offset."
            self.assertIn("no ```diff fenced block", messages[-1]["content"])
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        (built, _, _, _), _ = self._run(fake_call_model, lambda root: (True, "ok"))
        self.assertTrue(built)

    def test_verify_budget_exhaustion_demands_final_diff_then_fails_on_refusal(self):
        result, _ = self._run(
            lambda messages, *a: "VERIFY\n```diff\n--- a/x\n+++ b/x\n```\n",
            lambda root: (True, "ok"),
            config=dict(CONFIG, max_verify_turns=2),
        )
        built, reason, diff, messages = result
        self.assertFalse(built)
        self.assertIn("verify budget", reason)
```

- [ ] **Step 2: Run to verify failure**

Run: `python3 -m unittest test_model_fix_loop.CargoCheckTests test_model_fix_loop.AttemptBuildVerifyTests -v 2>&1 | tail -15`
Expected: ImportError (`cargo_check` not defined).

- [ ] **Step 3: Implement**

Next to `cargo_build` (~line 497), add:

```python
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
```

Next to `PATCH_HEADER_RE`, add:

```python
VERIFY_RE = re.compile(r"^VERIFY\b", re.IGNORECASE)
DEFAULT_MAX_VERIFY_TURNS = 10   # trial-compile turns per attempt_build invocation
DEFAULT_MAX_CHECK_OUTPUT_CHARS = 3000  # tail-trim: Rust errors summarize at the end
```

`attempt_build` signature gains `cargo_check_fn=None` (after `samples_dir=None`). Docstring: append this paragraph:

```
    cargo_check_fn(repo_root) -> (success, output), if provided, enables
    the VERIFY protocol: a reply of "VERIFY" plus one ```diff fenced
    block gets that diff applied, cargo-checked, REVERTED, and the
    tail-trimmed check output fed back -- a trial compile that never
    consumes one of the 2 real diff attempts. Bounded by
    config["max_verify_turns"] (default DEFAULT_MAX_VERIFY_TURNS).
    None (the default) keeps VERIFY off: such replies get an
    "unavailable" message, so old callers and tests are unaffected.
```

Pre-loop initialization additions:

```python
    max_verify_turns = config.get("max_verify_turns", DEFAULT_MAX_VERIFY_TURNS)
    verify_turns_used = 0
    verify_rejections = 0
```

Insert the VERIFY branch in the while loop AFTER the whole `if request_match:` block and BEFORE the `patch_match = PATCH_HEADER_RE.match(...)` line:

```python
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
            continue
```

In `fix_gap`: add `cargo_check_fn=cargo_check,` to the signature (after `cargo_test_workspace_fn=cargo_test_workspace,`), one docstring line (`cargo_check_fn is threaded to attempt_build_fn for the VERIFY protocol.`), and add `cargo_check_fn=cargo_check_fn,` to the `attempt_build_fn(...)` call's kwargs.

- [ ] **Step 4: Run new classes, then the full file**

Run: `python3 -m unittest test_model_fix_loop.CargoCheckTests test_model_fix_loop.AttemptBuildVerifyTests -v 2>&1 | tail -12` → all PASS.
Run: `python3 -m unittest test_model_fix_loop` → OK (fix_gap fakes absorb the new kwarg via `**kwargs`).

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: VERIFY incremental compile-check protocol

The fixer can trial-compile a candidate change before committing to it:
VERIFY + a fenced diff gets applied, cargo-checked (fast, no codegen),
reverted, and the tail-trimmed compiler output fed back -- never
consuming one of the 2 real diff attempts. Bounded by max_verify_turns
(default 10); gracefully unavailable when no cargo_check_fn is wired.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Context compaction (F6)

**Files:**
- Modify: `scripts/model_fix_loop.py` (new `compact_messages` next to `estimate_tokens`; `attempt_build` loop head)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Produces: `compact_messages(messages, trigger_tokens=DEFAULT_COMPACTION_TRIGGER_TOKENS, keep_recent=DEFAULT_COMPACTION_KEEP_RECENT_TURNS) -> list` (pure; returns a new list, original untouched); constants `DEFAULT_COMPACTION_TRIGGER_TOKENS = 12_000`, `DEFAULT_COMPACTION_KEEP_RECENT_TURNS = 4`, `DEFAULT_COMPACTION_MIN_ELIDE_TOKENS = 1000`. Config knobs `compaction_trigger_tokens`, `compaction_keep_recent_turns`. Stub text starts with `[earlier content elided for space:`.
- Consumes: `estimate_tokens` (Task-independent; already exists).

- [ ] **Step 1: Write the failing tests**

Add `compact_messages,` to the test import block. Add:

```python
class CompactMessagesTests(unittest.TestCase):
    def _messages(self):
        big = "x" * 8000   # ~2000 estimated tokens
        return [
            {"role": "user", "content": "initial prompt " + "p" * 8000},
            {"role": "assistant", "content": "REQUEST: src/a.rs"},
            {"role": "user", "content": "Contents of src/a.rs:\n" + big},
            {"role": "assistant", "content": "REQUEST: src/b.rs"},
            {"role": "user", "content": "Contents of src/b.rs:\n" + big},
            {"role": "assistant", "content": "```diff\n--- a/x\n+++ b/x\n```"},
            {"role": "user", "content": "That diff did not apply: whitespace\nPlease resend a corrected diff."},
        ]

    def test_below_trigger_returns_messages_unchanged(self):
        messages = self._messages()
        result = compact_messages(messages, trigger_tokens=10_000_000)
        self.assertEqual(result, messages)

    def test_above_trigger_stubs_old_large_user_turns_only(self):
        messages = self._messages()
        result = compact_messages(messages, trigger_tokens=100, keep_recent=2)
        # message 0 (initial prompt) is never touched
        self.assertEqual(result[0], messages[0])
        # assistant turns are never touched
        self.assertEqual(result[1], messages[1])
        self.assertEqual(result[3], messages[3])
        self.assertEqual(result[5], messages[5])
        # old large served payloads are stubbed
        self.assertIn("[earlier content elided for space:", result[2]["content"])
        self.assertIn("Contents of src/a.rs:", result[2]["content"])  # first line kept
        # the last keep_recent=2 messages are untouched
        self.assertEqual(result[5], messages[5])
        self.assertEqual(result[6], messages[6])

    def test_small_user_turns_are_not_stubbed(self):
        messages = self._messages()
        result = compact_messages(messages, trigger_tokens=100, keep_recent=0)
        # the small repair prompt (message 6) is under the elide floor
        self.assertEqual(result[6], messages[6])

    def test_compaction_is_idempotent(self):
        messages = self._messages()
        once = compact_messages(messages, trigger_tokens=100, keep_recent=2)
        twice = compact_messages(once, trigger_tokens=100, keep_recent=2)
        self.assertEqual(once, twice)

    def test_original_list_is_not_mutated(self):
        messages = self._messages()
        snapshot = [dict(m) for m in messages]
        compact_messages(messages, trigger_tokens=100, keep_recent=2)
        self.assertEqual(messages, snapshot)
```

Also add to `AttemptBuildTests`:

```python
    def test_conversation_is_compacted_when_over_the_trigger(self):
        big_answer = "Contents of src/a.rs:\n" + "y" * 60_000
        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) <= 2:
                return f"REQUEST: src/a{len(replies)}.rs"
            # By the 3rd call, the first served payload must be stubbed.
            stub_turns = [m for m in messages if "[earlier content elided for space:" in m["content"]]
            self.assertGreaterEqual(len(stub_turns), 1)
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        with tempfile.TemporaryDirectory() as tmpdir:
            repo = Path(tmpdir)
            (repo / "src").mkdir()
            (repo / "src" / "a1.rs").write_text("y" * 60_000)
            (repo / "src" / "a2.rs").write_text("y" * 60_000)
            built, reason, diff, messages = attempt_build(
                [{"role": "user", "content": "fix format X"}],
                call_model_fn=fake_call_model,
                git_apply_fn=lambda diff, root: (True, "ok"),
                git_checkout_clean_fn=lambda root: None,
                cargo_build_fn=lambda root: (True, ""),
                config=dict(CONFIG, compaction_trigger_tokens=5000, compaction_keep_recent_turns=1),
                repo_root=repo,
            )
        self.assertTrue(built)
```

- [ ] **Step 2: Run to verify failure**

Run: `python3 -m unittest test_model_fix_loop.CompactMessagesTests -v 2>&1 | tail -8`
Expected: ImportError (`compact_messages` not defined).

- [ ] **Step 3: Implement**

Next to `truncate_to_token_budget` in `model_fix_loop.py`:

```python
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
```

In `attempt_build`, at the very top of the `while diff_attempts_used < 2:` loop body (before `model_spec = pick_model_fn(...)`):

```python
        messages[:] = compact_messages(
            messages,
            trigger_tokens=config.get("compaction_trigger_tokens", DEFAULT_COMPACTION_TRIGGER_TOKENS),
            keep_recent=config.get("compaction_keep_recent_turns", DEFAULT_COMPACTION_KEEP_RECENT_TURNS),
        )
```

(Slice-assignment because `messages` is shared with the caller and returned.)

- [ ] **Step 4: Run new tests, then the full file**

Run: `python3 -m unittest test_model_fix_loop.CompactMessagesTests -v` → all PASS.
Run: `python3 -m unittest test_model_fix_loop` → OK.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: context compaction for long fixer conversations

Once the conversation exceeds compaction_trigger_tokens (default 12k
estimated), stale large served payloads (REQUEST answers, VERIFY
outputs) are stubbed to one line. The initial prompt, the most recent
turns, and every assistant message survive verbatim.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Tiered explore/patch model routing + per-entry reasoning effort (F8)

**Files:**
- Modify: `scripts/model_fix_loop.py` (`_KNOWN_MODEL_SPEC_KEYS` + `_normalize_model_spec` ~line 2198; new `models_for_phase` helper next to them; `attempt_build` phase state; `critique_failed_attempt`; `summarize_format_memory`; `review_verdict` — per-call reasoning effort at every picked-spec call site)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Produces: `models_for_phase(models, phase) -> list`; model-spec dicts now carry optional `"phase"` (`"explore"`/`"patch"`/None) and `"reasoning_effort"` (str|None); `_VALID_MODEL_PHASES = {"explore", "patch"}`. Every call site computes effort as `model_spec.get("reasoning_effort") or config["reasoning_effort"]`.
- Consumes: `attempt_build`'s reply-type branches (REQUEST/VERIFY/PATCH/diff) from Tasks 3-4.

- [ ] **Step 1: Write the failing tests**

Add `models_for_phase,` to the test import block. Add:

```python
class ModelsForPhaseTests(unittest.TestCase):
    TERRA = {"name": "gpt-5.6-terra", "base_url": "u", "api_key": "k", "phase": "explore", "reasoning_effort": "medium"}
    SOL = {"name": "gpt-5.6-sol", "base_url": "u", "api_key": "k", "phase": "patch", "reasoning_effort": "max"}
    UNTAGGED = {"name": "any", "base_url": "u", "api_key": "k"}

    def test_filters_to_matching_phase(self):
        pool = [self.TERRA, self.SOL]
        self.assertEqual(models_for_phase(pool, "explore"), [self.TERRA])
        self.assertEqual(models_for_phase(pool, "patch"), [self.SOL])

    def test_untagged_entries_belong_to_every_phase(self):
        pool = [self.TERRA, self.UNTAGGED]
        self.assertEqual(models_for_phase(pool, "patch"), [self.UNTAGGED])
        self.assertEqual(models_for_phase(pool, "explore"), [self.TERRA, self.UNTAGGED])

    def test_empty_filter_falls_back_to_full_pool(self):
        pool = [self.TERRA]
        self.assertEqual(models_for_phase(pool, "patch"), [self.TERRA])


class ModelSpecPhaseTests(unittest.TestCase):
    def test_phase_and_reasoning_effort_are_accepted(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k",
            "models": [{"name": "m", "phase": "explore", "reasoning_effort": "medium"}],
        })
        self.assertEqual(config["models"][0]["phase"], "explore")
        self.assertEqual(config["models"][0]["reasoning_effort"], "medium")

    def test_missing_phase_and_effort_default_to_none(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["bare-name"],
        })
        self.assertIsNone(config["models"][0].get("phase"))
        self.assertIsNone(config["models"][0].get("reasoning_effort"))

    def test_invalid_phase_raises_at_load(self):
        with self.assertRaises(ValueError):
            _normalize_model_config({
                "base_url": "u", "api_key": "k",
                "models": [{"name": "m", "phase": "turbo"}],
            })

    def test_unknown_key_still_raises(self):
        with self.assertRaises(ValueError):
            _normalize_model_config({
                "base_url": "u", "api_key": "k",
                "models": [{"name": "m", "max_tokens": 4096}],
            })


class AttemptBuildPhaseRoutingTests(unittest.TestCase):
    TERRA = {"name": "gpt-5.6-terra", "base_url": "u", "api_key": "k", "phase": "explore", "reasoning_effort": "medium"}
    SOL = {"name": "gpt-5.6-sol", "base_url": "u", "api_key": "k", "phase": "patch", "reasoning_effort": "max"}

    def test_explore_then_patch_pools_across_a_repair(self):
        pools_seen = []

        def tracking_pick(models):
            pools_seen.append([m["name"] for m in models])
            return models[0]

        replies = []

        def fake_call_model(messages, *a):
            replies.append(1)
            if len(replies) == 1:
                return "REQUEST: src/a.rs"
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        apply_results = iter([(False, "does not apply"), (True, "ok")])
        built, reason, diff, messages = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: next(apply_results),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=dict(CONFIG, models=[self.TERRA, self.SOL]),
            repo_root=Path("/fake/repo"),
            pick_model_fn=tracking_pick,
        )
        self.assertTrue(built)
        # Call 1: fresh attempt -> explore (terra). Call 2: after a served
        # REQUEST answer -> still explore. Call 3: after an apply-failure
        # repair prompt -> patch (sol).
        self.assertEqual(pools_seen[0], ["gpt-5.6-terra"])
        self.assertEqual(pools_seen[1], ["gpt-5.6-terra"])
        self.assertEqual(pools_seen[2], ["gpt-5.6-sol"])

    def test_reinvocation_with_existing_conversation_starts_in_patch_phase(self):
        pools_seen = []

        def tracking_pick(models):
            pools_seen.append([m["name"] for m in models])
            return models[0]

        built, reason, diff, messages = attempt_build(
            [
                {"role": "user", "content": "fix format X"},
                {"role": "assistant", "content": "```diff\nbad\n```"},
                {"role": "user", "content": "That attempt failed (build_failed): ...\nPlease resend a corrected diff."},
            ],
            call_model_fn=lambda messages, *a: "```diff\n--- a/x\n+++ b/x\n```\n",
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=dict(CONFIG, models=[self.TERRA, self.SOL]),
            repo_root=Path("/fake/repo"),
            pick_model_fn=tracking_pick,
        )
        self.assertTrue(built)
        self.assertEqual(pools_seen[0], ["gpt-5.6-sol"])

    def test_per_entry_reasoning_effort_reaches_the_call(self):
        efforts_seen = []

        def fake_call_model(messages, base_url, api_key, model, max_tokens, reasoning_effort, *a):
            efforts_seen.append(reasoning_effort)
            return "```diff\n--- a/x\n+++ b/x\n```\n"

        built, *_ = attempt_build(
            [{"role": "user", "content": "fix format X"}],
            call_model_fn=fake_call_model,
            git_apply_fn=lambda diff, root: (True, "ok"),
            git_checkout_clean_fn=lambda root: None,
            cargo_build_fn=lambda root: (True, ""),
            config=dict(CONFIG, models=[self.TERRA]),
            repo_root=Path("/fake/repo"),
        )
        self.assertTrue(built)
        self.assertEqual(efforts_seen, ["medium"])


class CritiqueUsesExploreTierTests(unittest.TestCase):
    def test_critique_picks_from_the_explore_pool(self):
        terra = {"name": "terra", "base_url": "u", "api_key": "k", "phase": "explore"}
        sol = {"name": "sol", "base_url": "u", "api_key": "k", "phase": "patch"}
        pools_seen = []

        def tracking_pick(models):
            pools_seen.append([m["name"] for m in models])
            return models[0]

        config = dict(CONFIG, models=[terra, sol])
        critique = critique_failed_attempt(
            make_gap(gap_count=1), "--- a/x\n", "build_failed", "error", config,
            call_model_fn=lambda *a: "try a different offset",
            pick_model_fn=tracking_pick,
        )
        self.assertEqual(critique, "try a different offset")
        self.assertEqual(pools_seen, [["terra"]])
```

- [ ] **Step 2: Run to verify failure**

Run: `python3 -m unittest test_model_fix_loop.ModelsForPhaseTests -v 2>&1 | tail -5`
Expected: ImportError (`models_for_phase` not defined).

- [ ] **Step 3: Implement**

Replace `_KNOWN_MODEL_SPEC_KEYS = {"name", "base_url", "api_key"}` with:

```python
_KNOWN_MODEL_SPEC_KEYS = {"name", "base_url", "api_key", "phase", "reasoning_effort"}
_VALID_MODEL_PHASES = {"explore", "patch"}


def models_for_phase(models, phase):
    """Filter a model pool to entries tagged for `phase` -- untagged
    entries (phase absent/None) are eligible for every phase. Falls back
    to the full pool when the filter would be empty, so a config with no
    phase tags behaves exactly as before this feature existed."""
    matching = [m for m in models if m.get("phase") in (None, phase)]
    return matching or models
```

In `_normalize_model_spec`: extend the string-entry return to `{"name": entry, "base_url": default_base_url, "api_key": default_api_key, "phase": None, "reasoning_effort": None}`. In the table-entry path, after the unknown-key check add:

```python
    phase = entry.get("phase")
    if phase is not None and phase not in _VALID_MODEL_PHASES:
        raise ValueError(
            f"invalid phase {phase!r} on models[] entry {entry.get('name', '?')!r} -- "
            f"must be one of {sorted(_VALID_MODEL_PHASES)} (or omitted for both phases)"
        )
```

and extend the returned dict with `"phase": phase, "reasoning_effort": entry.get("reasoning_effort"),`. Update the unknown-key error message's "only name/base_url/api_key belong" phrasing to "only name/base_url/api_key/phase/reasoning_effort belong". Update the docstring's key list sentence the same way.

In `attempt_build`: after the pre-loop initialization add `current_phase = "explore" if len(messages) == 1 else "patch"`. Change the pick line to:

```python
        model_spec = pick_model_fn(models_for_phase(config["models"], current_phase))
```

Change the `call_model_fn(...)` invocation's effort argument from `config["reasoning_effort"]` to `model_spec.get("reasoning_effort") or config["reasoning_effort"]`.

Set `current_phase` before each `continue`/loop-bottom according to what the just-appended user turn demands:
- After serving a REQUEST answer: `current_phase = "explore"`
- After the dead-end pivot nudge (Task 3): `current_phase = "patch"`
- After the exhausted-budget "No more file requests" nudge: `current_phase = "patch"`
- In every VERIFY sub-branch (result, malformed, apply-failure, unavailable/budget message): `current_phase = "explore"`
- After every PATCH-chunk prompt (missing-chunk request, malformed-chunk resend): `current_phase = "patch"`
- After the apply-failure repair prompt and the build-failure repair prompt (bottom of loop): `current_phase = "patch"`

In `critique_failed_attempt`: change `model_spec = pick_model_fn(config["models"])` to `model_spec = pick_model_fn(models_for_phase(config["models"], "explore"))` and the effort argument to `model_spec.get("reasoning_effort") or config["reasoning_effort"]`.

In `summarize_format_memory` (~line 992) and `review_verdict`: apply the same two changes — `models_for_phase(config["models"], "explore")` for summarize, and for `review_verdict` keep the full pool (`pick_model_fn(config["models"])` unchanged — the reviewer has its own `[reviewer]` config table) but still switch its effort argument to `model_spec.get("reasoning_effort") or config["reasoning_effort"]`.

- [ ] **Step 4: Run new classes, then the full file**

Run: `python3 -m unittest test_model_fix_loop.ModelsForPhaseTests test_model_fix_loop.ModelSpecPhaseTests test_model_fix_loop.AttemptBuildPhaseRoutingTests test_model_fix_loop.CritiqueUsesExploreTierTests -v 2>&1 | tail -12` → all PASS.
Run: `python3 -m unittest test_model_fix_loop` → OK. (The pre-existing `test_picks_a_model_from_the_pool_for_each_call_via_pick_model_fn` asserts pool equality — `models_for_phase` on untagged specs returns an equal list, so it still passes.)

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: tiered explore/patch model routing + per-entry reasoning effort

[[worker.models]] entries can tag phase = explore|patch and override
reasoning_effort per model. attempt_build routes investigation turns
(fresh attempt, post-REQUEST, post-VERIFY) to the explore tier and every
diff-demanding turn (nudges, PATCH continuations, repairs, critique
retries) to the patch tier; critique and memory-summarization calls use
the explore tier. Untagged configs behave exactly as before.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Reply-shape manifest + cache-friendly prompt ordering (F2 + F7)

**Files:**
- Modify: `scripts/model_fix_loop.py` (`build_prompt` assembly + `samples_block` text; new `build_reply_shape_manifest` + `TERMINAL_REMINDER` near `KNOWN_PITFALLS`)
- Test: `scripts/test_model_fix_loop.py` (new ordering tests; update existing prompt-text assertions)

**Interfaces:**
- Consumes: `RUST_ARCHITECTURE_CONSTRAINTS` (Task 1), VERIFY protocol (Task 4), range syntax (Task 2), PATCH i/N (existing).
- Produces: `build_reply_shape_manifest(max_prompt_tokens) -> str` (contains the literal strings `REQUEST:`, `VERIFY`, `PATCH 1/N`, `Plan + diff`, `exactly one of these four shapes`, and `roughly {max_prompt_tokens} tokens`); `TERMINAL_REMINDER` constant ending with ``plan + a single ```diff block.``; reordered `build_prompt` (static → gap → volatile → reminder).

- [ ] **Step 1: Write the failing tests**

Add `build_reply_shape_manifest,` and `TERMINAL_REMINDER,` to the test import block. Add:

```python
class BuildPromptOrderingTests(unittest.TestCase):
    def test_static_sections_precede_gap_content(self):
        prompt = build_prompt(make_gap(gap_count=2))
        gap_pos = prompt.index("Missing entirely")
        self.assertLess(prompt.index("CRITICAL RUST ARCHITECTURE CONSTRAINTS"), gap_pos)
        self.assertLess(prompt.index("Lessons from mistakes"), gap_pos)
        self.assertLess(prompt.index("exactly one of these four shapes"), gap_pos)

    def test_volatile_history_comes_after_gap_content(self):
        attempts = [{"diff": "--- a/x\n", "status": "failed", "reason": "build failed"}]
        prompt = build_prompt(make_gap(gap_count=2), previous_attempts=attempts)
        self.assertGreater(
            prompt.index("Previous attempts on this exact tag"),
            prompt.index("Missing entirely"),
        )

    def test_terminal_reminder_is_the_last_line(self):
        prompt = build_prompt(make_gap(gap_count=2))
        self.assertTrue(prompt.rstrip().endswith(TERMINAL_REMINDER.rstrip()))

    def test_manifest_lists_all_four_shapes_and_range_syntax(self):
        manifest = build_reply_shape_manifest(4096)
        for needle in ("REQUEST:", "VERIFY", "PATCH 1/N", "Plan + diff",
                       ":<start>-<end>", "roughly 4096 tokens", "ephemeral"):
            self.assertIn(needle, manifest)
```

- [ ] **Step 2: Run to verify failure**

Run: `python3 -m unittest test_model_fix_loop.BuildPromptOrderingTests -v 2>&1 | tail -6`
Expected: ImportError (`build_reply_shape_manifest` not defined).

- [ ] **Step 3: Implement**

After the `KNOWN_PITFALLS` constant in `model_fix_loop.py`:

```python
def build_reply_shape_manifest(max_prompt_tokens):
    """The complete reply protocol, stated once near the top of the
    prompt (stable text -> provider prompt-cache friendly; early text ->
    survives truncate_to_token_budget, which keeps the head)."""
    return f"""You are operating in an ephemeral, isolated git worktree; broken builds during investigation are expected and cost nothing -- probe aggressively with VERIFY rather than guessing.

Every reply must be EXACTLY one of these four shapes:

1. REQUEST: <path> -- see a source file or a sample file (a bare line, nothing else in the reply). Add :<start>-<end> after a source path (e.g. REQUEST: src/parsers/x.rs:40-120) to get just that 1-indexed line range -- prefer a range for anything large.
2. VERIFY -- trial-compile a candidate change without committing to it: the line "VERIFY" followed by exactly ONE ```diff fenced block. The diff is applied, `cargo check` runs, the tail of its output comes back, and the change is REVERTED -- your final diff must still contain the complete change.
3. PATCH 1/N -- if your finished diff would exceed roughly {max_prompt_tokens} tokens (~{max_prompt_tokens * 4} characters) in one reply, split it into N consecutive chunks and send the first as the line "PATCH 1/N" followed by ONE ```diff fenced chunk; you'll be prompted for each next chunk. Chunks are concatenated in order before applying, so split anywhere (mid-hunk is fine) -- never repeat or skip lines across a boundary.
4. Plan + diff -- first, 2-3 sentences: which tag(s) you're fixing, where in the code, what you learned from the previous turn's output, and (on a retry) what you're doing differently from the failed attempt(s) above and why. Then exactly ONE ```diff fenced block containing the complete unified diff.

Shapes 1-3 are control signals: the control line must be the VERY FIRST line of the reply, with no narrative before it."""


TERMINAL_REMINDER = (
    "Reply now with exactly one of the four shapes defined at the top: "
    "REQUEST, VERIFY, PATCH i/N, or plan + a single ```diff block."
)
```

Rewrite `build_prompt`'s final assembly (replacing everything from `prompt = (` through `return truncate_to_token_budget(prompt, max_prompt_tokens)`) with:

```python
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
```

Also shrink `samples_block`'s embedded protocol prose (it currently re-explains REQUEST). Replace its message text with:

```python
            samples_block = (
                f"\n\nReal sample files available for this format (relative to the samples dir):\n{listed}\n"
                "(REQUEST one -- shape 1 above -- to get a hex dump of its raw bytes.)"
            )
```

Update the `build_prompt` docstring's mention of ordering: append one sentence — `Sections are ordered static-first (constraints/pitfalls/manifest), then per-tag content, then volatile history, so the byte-stable prefix is maximal for provider prompt caching and survives head-keeping truncation.`

- [ ] **Step 4: Run the full file; repair any prompt-text assertions**

Run: `python3 -m unittest test_model_fix_loop 2>&1 | tail -20`

Expected breakage to repair (semantics preserved, strings updated):
- `BuildPromptTokenBudgetTests.test_always_explains_the_patch_chunking_protocol` — still passes (`PATCH 1/N` and `` ```diff `` both appear in the manifest). Verify.
- `BuildPromptTokenBudgetTests.test_mentions_the_configured_token_budget` — still passes (`roughly 4096 tokens`). Verify.
- Any test asserting the OLD closing-instruction strings (`"REQUEST: <path>\" line described above"`, `"state your plan"`, `"single unified diff"`): update each to assert the manifest equivalents (`"exactly one of these four shapes"`, `"2-3 sentences"`, `"complete unified diff"`). Search: `grep -n "state your plan\|described above\|single unified diff" test_model_fix_loop.py`.
- `BuildExactSampleBlockTests` / sample-listing tests asserting the old samples_block sentence (`"If you need to see actual raw bytes"`): update to the new one-liner (`"shape 1 above"`).

Then: `python3 -m unittest test_model_fix_loop` → OK, and `python3 -m unittest discover -p "test_*.py"` → OK.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: four-shape reply manifest + cache-friendly prompt ordering

One protocol block near the top of every prompt (REQUEST with optional
line ranges / VERIFY / PATCH i-of-N / plan+diff), shadow-worktree
framing, and a tiny stable terminal reminder. Prompt sections reordered
static-first -> per-tag -> volatile-history so the byte-stable prefix
is maximal for provider prompt caching and survives head-keeping
truncation.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Config knobs, TOML defaults, docs, full-suite gate

**Files:**
- Modify: `scripts/model_fix_loop.py` (`_normalize_model_config` ~line 2235; module docstring's config table)
- Modify: `config.example.toml` (repo root of the worktree)
- Modify: `config.toml` (gitignored — live copy in this worktree)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Consumes: every default constant from Tasks 3-6 (`DEFAULT_MAX_REQUEST_REPEATS`, `DEFAULT_MAX_VERIFY_TURNS`, `DEFAULT_COMPACTION_TRIGGER_TOKENS`, `DEFAULT_COMPACTION_KEEP_RECENT_TURNS`).
- Produces: `_normalize_model_config` output carrying `max_request_repeats`, `max_verify_turns`, `compaction_trigger_tokens`, `compaction_keep_recent_turns`.

- [ ] **Step 1: Write the failing test**

Add to the existing `_normalize_model_config` test class (search: `grep -n "class.*NormalizeModelConfig\|_normalize_model_config" test_model_fix_loop.py` — add to the class that already tests `max_repair_rounds` defaults):

```python
    def test_new_harness_knobs_have_defaults(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["max_request_repeats"], 3)
        self.assertEqual(config["max_verify_turns"], 10)
        self.assertEqual(config["compaction_trigger_tokens"], 12_000)
        self.assertEqual(config["compaction_keep_recent_turns"], 4)

    def test_new_harness_knobs_are_overridable(self):
        config = _normalize_model_config({
            "base_url": "u", "api_key": "k", "models": ["m"],
            "max_request_repeats": 5, "max_verify_turns": 2,
            "compaction_trigger_tokens": 6000, "compaction_keep_recent_turns": 8,
        })
        self.assertEqual(config["max_request_repeats"], 5)
        self.assertEqual(config["max_verify_turns"], 2)
        self.assertEqual(config["compaction_trigger_tokens"], 6000)
        self.assertEqual(config["compaction_keep_recent_turns"], 8)
```

- [ ] **Step 2: Run to verify failure**

Run: `python3 -m unittest test_model_fix_loop -k new_harness_knobs -v`
Expected: FAIL (KeyError on the new keys).

- [ ] **Step 3: Implement**

In `_normalize_model_config`'s returned dict, after the `"max_repair_rounds"` line add:

```python
        "max_request_repeats": table.get("max_request_repeats", DEFAULT_MAX_REQUEST_REPEATS),
        "max_verify_turns": table.get("max_verify_turns", DEFAULT_MAX_VERIFY_TURNS),
        "compaction_trigger_tokens": table.get("compaction_trigger_tokens", DEFAULT_COMPACTION_TRIGGER_TOKENS),
        "compaction_keep_recent_turns": table.get("compaction_keep_recent_turns", DEFAULT_COMPACTION_KEEP_RECENT_TURNS),
```

In the module docstring's config table (top of file), after the `max_prompt_tokens` entry add:

```
    max_request_repeats    default 3 (worker only; identical REQUESTs before
                          a pivot nudge replaces the served content)
    max_verify_turns       default 10 (worker only; VERIFY trial-compile
                          turns per attempt -- see attempt_build)
    compaction_trigger_tokens      default 12000; conversation size (est.
                          tokens) beyond which stale served payloads are
                          stubbed -- see compact_messages
    compaction_keep_recent_turns   default 4; most-recent messages exempt
                          from compaction
```

Also document per-entry keys in the docstring's models description: append `Each entry may also set phase = "explore"|"patch" (which conversation turns it serves -- see models_for_phase) and reasoning_effort (per-model override of the table default).`

In `config.example.toml`, under `[worker]` after `max_prompt_tokens = 4096` add:

```toml
max_request_repeats = 3
max_verify_turns = 10
compaction_trigger_tokens = 12000
compaction_keep_recent_turns = 4
```

and REPLACE the example `[[worker.models]]` entries with the tiered pair (keeping the Fireworks mixed-provider example commented out or removing it):

```toml
# Tiered routing: explore-phase turns (investigation, REQUEST/VERIFY
# round-trips) use the cheap fast model; patch-phase turns (final diffs,
# repairs, retries) use the strongest model. Omit "phase" on an entry to
# use it for both.
[[worker.models]]
name = "gpt-5.6-terra"
phase = "explore"
reasoning_effort = "medium"

[[worker.models]]
name = "gpt-5.6-sol"
phase = "patch"
reasoning_effort = "max"
```

In the live `config.toml` (same directory — gitignored, edit but do not `git add`), make the same two changes under `[worker]` (the four knobs + replace the single `[[worker.models]] name = "gpt-5.6-sol"` entry with the terra/sol pair exactly as above). Leave `[reviewer]` as-is (already sol @ max).

- [ ] **Step 4: Run the gates**

Run: `python3 -m unittest test_model_fix_loop -k new_harness_knobs -v` → PASS.
Run: `python3 -m unittest discover -p "test_*.py" 2>&1 | tail -3` → OK (405 pre-existing + all new).
Sanity-load the live config: `python3 -c "import model_fix_loop as m; import tomllib; d = m.load_toml_config(m.DEFAULT_CONFIG_PATH); c = m._normalize_model_config(d['worker']); print([ (x['name'], x.get('phase'), x.get('reasoning_effort')) for x in c['models'] ], c['max_verify_turns'])"` → prints the terra/sol pair with phases/efforts and `10`.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py config.example.toml
git commit -m "feat: config knobs + tiered terra/sol defaults for the hardened harness

max_request_repeats / max_verify_turns / compaction_* normalized with
defaults; config.example.toml documents the tiered model pool
(gpt-5.6-terra explore@medium, gpt-5.6-sol patch@max). Live config.toml
(gitignored) updated to match.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Final integration (performed by the orchestrator, not a task subagent)

1. `python3 -m unittest discover -p "test_*.py"` one last time in the worktree.
2. Push `feat/model-fix-loop-context` → PR #41; comment summarizing the feature set.
3. Merge into local `main` (`/Users/allen/git/oxidex`), run the discover suite there.
4. Propagate `scripts/model_fix_loop.py` AND `config.toml` to all 20 live worker worktrees (`~/.oxidex/worktrees/parallel-fix/model-fix-*`).
5. Note to user: dispatcher restart required to pick up code+config.
