# Throughput (Tags Per Hour) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Raise tags/hour via a cross-process rate governor, sibling-tag clustering, differential target-tag verification, neighbor-precedent prompting, cheaper test rounds, and a landed-tags skip set.

**Architecture:** All logic in `scripts/model_fix_loop.py` (+ `scripts/log_sweep_review.py` for one task), pure/injectable functions, hermetic tests. Tasks strictly sequential — shared files.

**Tech Stack:** Python 3.11+ stdlib only (`fcntl` for file locks — macOS/Linux fine), unittest.

**Spec:** `docs/plans/specs/2026-07-23-throughput-tags-per-hour-design.md` — read it first.

## Global Constraints

- Working dir for tests: `/Users/allen/.oxidex/worktrees/sweep-tags/scripts`; commits from `/Users/allen/.oxidex/worktrees/sweep-tags` (branch `feat/model-fix-loop-context`). NEVER push.
- Suites: `python3 -m unittest test_model_fix_loop` and `python3 -m unittest discover -p "test_*.py"` — discover currently passes with **466 tests**; must pass after every task with new tests added.
- No new dependencies (`dependencies = []` uv scripts).
- Every commit message ends with `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- All new config knobs read via `config.get("<name>", DEFAULT_<NAME>)`.
- A concurrent session occasionally edits these files. Before starting, run `git -C /Users/allen/.oxidex/worktrees/sweep-tags status --short` — if a file you must edit is dirty, STOP and report rather than committing someone else's work. Only commit paths you changed.

---

### Task 1: Rate-governor core (`governor_acquire` / `governor_report`)

**Files:**
- Modify: `scripts/model_fix_loop.py` (new section after `cargo_env`, ~line 610)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces (produces):**
- `DEFAULT_GOVERNOR_PATH = OXIDEX_HOME / "logs" / "rate-governor.json"`
- `DEFAULT_GOVERNOR_CALLS_PER_MINUTE = 30`, `DEFAULT_GOVERNOR_BURST = 5`, `DEFAULT_GOVERNOR_COOLDOWN_SECONDS = 30`, `DEFAULT_GOVERNOR_MAX_COOLDOWN_SECONDS = 300`
- `governor_acquire(path, calls_per_minute=..., burst=..., now_fn=time.time, sleep_fn=time.sleep, jitter_fn=random.random) -> None`
- `governor_report(path, limited, cooldown_seconds=..., max_cooldown_seconds=..., now_fn=time.time) -> None`
- Both no-ops when `path is None`.

- [ ] **Step 1: Write the failing tests**

Add `governor_acquire, governor_report,` to the test import block (alphabetical). Add:

```python
class RateGovernorTests(unittest.TestCase):
    def setUp(self):
        self.tmpdir = tempfile.TemporaryDirectory()
        self.path = Path(self.tmpdir.name) / "rate-governor.json"

    def tearDown(self):
        self.tmpdir.cleanup()

    def test_none_path_is_a_noop(self):
        governor_acquire(None)  # must not raise or sleep
        governor_report(None, limited=True)

    def test_burst_tokens_allow_immediate_calls_then_throttle(self):
        clock = [1000.0]
        sleeps = []

        def now():
            return clock[0]

        def sleep(s):
            sleeps.append(s)
            clock[0] += s

        for _ in range(5):  # burst = 5 -> all immediate
            governor_acquire(self.path, calls_per_minute=60, burst=5,
                             now_fn=now, sleep_fn=sleep, jitter_fn=lambda: 0.5)
        self.assertEqual(sleeps, [])
        # 6th call: bucket empty, refill is 1/sec -> must wait ~1s
        governor_acquire(self.path, calls_per_minute=60, burst=5,
                         now_fn=now, sleep_fn=sleep, jitter_fn=lambda: 0.5)
        self.assertEqual(len(sleeps), 1)
        self.assertGreater(sleeps[0], 0)
        self.assertLess(sleeps[0], 2.5)

    def test_report_limited_sets_global_cooldown_acquire_waits_it_out(self):
        clock = [1000.0]
        sleeps = []

        def now():
            return clock[0]

        def sleep(s):
            sleeps.append(s)
            clock[0] += s

        governor_report(self.path, limited=True, cooldown_seconds=30,
                        max_cooldown_seconds=300, now_fn=now)
        governor_acquire(self.path, calls_per_minute=60, burst=5,
                         now_fn=now, sleep_fn=sleep, jitter_fn=lambda: 0.5)
        self.assertTrue(sleeps)
        self.assertGreaterEqual(sum(sleeps), 30 * 0.8)  # jitter can shave 20%

    def test_consecutive_limited_reports_grow_the_cooldown_capped(self):
        now_fn = lambda: 1000.0
        for _ in range(10):
            governor_report(self.path, limited=True, cooldown_seconds=30,
                            max_cooldown_seconds=120, now_fn=now_fn)
        state = json.loads(self.path.read_text())
        self.assertLessEqual(state["cooldown_until"], 1000.0 + 120)
        self.assertGreaterEqual(state["consecutive_limited"], 10)

    def test_success_resets_the_streak(self):
        now_fn = lambda: 1000.0
        governor_report(self.path, limited=True, now_fn=now_fn)
        governor_report(self.path, limited=False, now_fn=now_fn)
        state = json.loads(self.path.read_text())
        self.assertEqual(state["consecutive_limited"], 0)

    def test_corrupt_state_file_recovers_permissively(self):
        self.path.write_text("{not json")
        governor_acquire(self.path, calls_per_minute=60, burst=5,
                         now_fn=lambda: 1000.0, sleep_fn=lambda s: None,
                         jitter_fn=lambda: 0.5)  # must not raise
        governor_report(self.path, limited=False, now_fn=lambda: 1000.0)
        json.loads(self.path.read_text())  # now valid again
```

- [ ] **Step 2: Run to verify failure**

Run: `python3 -m unittest test_model_fix_loop.RateGovernorTests -v 2>&1 | tail -4`
Expected: ImportError (`governor_acquire` not defined).

- [ ] **Step 3: Implement**

Add `import fcntl` and `import random` to the import block if absent (check first — `random` is already imported). After `cargo_env()` (~line 610):

```python
DEFAULT_GOVERNOR_PATH = OXIDEX_HOME / "logs" / "rate-governor.json"
DEFAULT_GOVERNOR_CALLS_PER_MINUTE = 30
DEFAULT_GOVERNOR_BURST = 5
DEFAULT_GOVERNOR_COOLDOWN_SECONDS = 30
DEFAULT_GOVERNOR_MAX_COOLDOWN_SECONDS = 300


def _governor_locked(path, mutate_fn, now_fn):
    """Run mutate_fn(state) -> (new_state, result) under an exclusive
    flock on path's sibling lockfile, loading/saving the JSON state
    around it. A missing or corrupt state file becomes a fresh
    permissive state -- the governor must never brick the loop over its
    own bookkeeping."""
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    lock_path = path.with_suffix(".lock")
    with open(lock_path, "w") as lock_f:
        fcntl.flock(lock_f, fcntl.LOCK_EX)
        try:
            state = json.loads(path.read_text())
            if not isinstance(state, dict):
                raise ValueError("state is not a dict")
        except (OSError, ValueError):
            state = {}
        state.setdefault("tokens", float(DEFAULT_GOVERNOR_BURST))
        state.setdefault("last_refill", now_fn())
        state.setdefault("cooldown_until", 0.0)
        state.setdefault("consecutive_limited", 0)
        new_state, result = mutate_fn(state)
        path.write_text(json.dumps(new_state))
        return result


def governor_acquire(path, calls_per_minute=DEFAULT_GOVERNOR_CALLS_PER_MINUTE,
                     burst=DEFAULT_GOVERNOR_BURST, now_fn=time.time,
                     sleep_fn=time.sleep, jitter_fn=random.random):
    """Block until this process may make one model API call.

    Cross-process token bucket + global cooldown, shared by every worker
    through one flock-guarded JSON file: refill at calls_per_minute/60
    tokens/sec (capped at burst), spend one per call, and honor
    cooldown_until -- which governor_report sets GLOBALLY on a 429/5xx,
    so one worker being limited pauses the whole fleet instead of the
    other N-1 continuing to hammer the shared account limit (measured
    today: 20 independent backoffs -> 13k 429s and zero successes in an
    hour). Waits carry +/-20% jitter so workers don't all wake at the
    same instant. path=None disables (old callers, tests).
    """
    if path is None:
        return
    while True:
        def try_take(state):
            now = now_fn()
            rate = calls_per_minute / 60.0
            elapsed = max(0.0, now - state["last_refill"])
            state["tokens"] = min(float(burst), state["tokens"] + elapsed * rate)
            state["last_refill"] = now
            if now < state["cooldown_until"]:
                return state, state["cooldown_until"] - now
            if state["tokens"] < 1.0:
                return state, (1.0 - state["tokens"]) / rate
            state["tokens"] -= 1.0
            return state, None

        wait = _governor_locked(path, try_take, now_fn)
        if wait is None:
            return
        sleep_fn(wait * (0.8 + 0.4 * jitter_fn()))


def governor_report(path, limited, cooldown_seconds=DEFAULT_GOVERNOR_COOLDOWN_SECONDS,
                    max_cooldown_seconds=DEFAULT_GOVERNOR_MAX_COOLDOWN_SECONDS,
                    now_fn=time.time):
    """Record one call outcome. limited=True (429 or 5xx) sets/extends
    the GLOBAL cooldown with exponential growth per consecutive limited
    outcome, capped; limited=False resets the streak (the next limited
    outcome starts from the base cooldown again). path=None disables."""
    if path is None:
        return
    def mutate(state):
        if limited:
            state["consecutive_limited"] += 1
            backoff = min(
                cooldown_seconds * (2 ** (state["consecutive_limited"] - 1)),
                max_cooldown_seconds,
            )
            state["cooldown_until"] = max(state["cooldown_until"], now_fn() + backoff)
        else:
            state["consecutive_limited"] = 0
        return state, None
    _governor_locked(path, mutate, now_fn)
```

- [ ] **Step 4: Run tests**

`python3 -m unittest test_model_fix_loop.RateGovernorTests -v` → all PASS.
`python3 -m unittest test_model_fix_loop` → OK. `python3 -m unittest discover -p "test_*.py"` → OK.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: cross-process rate governor (shared token bucket + global cooldown)

One flock-guarded state file all workers consult before every model
call: token-bucket pacing plus a GLOBAL cooldown any 429/5xx extends
exponentially -- so one worker being limited pauses the fleet instead
of the other N-1 continuing to hammer the shared account limit.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 2: Wire the governor into `call_model` (+ make 429 retryable) and config

**Files:**
- Modify: `scripts/model_fix_loop.py` (`DEFAULT_RETRYABLE_HTTP_STATUSES` ~line 170; `call_model` ~line 307; `_normalize_model_config`; `main()`'s `make_logging_call_model` threading)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- `call_model(..., governor_path=None, governor_calls_per_minute=..., governor_burst=..., governor_cooldown_seconds=..., governor_max_cooldown_seconds=...)` — keyword-only additions at the end of the signature; default `None` path keeps every existing caller/test byte-identical in behavior.
- `DEFAULT_RETRYABLE_HTTP_STATUSES = {429, 500, 502, 503, 504}` — 429 becomes retryable because the governor now paces retries globally; the infra-failure carve-out (Task earlier session) already keeps 429s from charging tags.
- `_normalize_model_config` gains `governor_calls_per_minute/burst/cooldown_seconds/max_cooldown_seconds` with the Task 1 defaults.

- [ ] **Step 1: Write the failing tests**

Check first: `grep -n "429" test_model_fix_loop.py` — if any existing CallModelRetryTests asserts a 429 raises immediately, change that test's code to 404 (the semantics it actually tests: non-retryable 4xx). Then add to `CallModelRetryTests` (mirroring its existing `_http_error` helper and patch style):

```python
    @patch("model_fix_loop.urllib.request.urlopen")
    def test_429_is_retried(self, mock_urlopen):
        ok_body = json.dumps({"choices": [{"message": {"content": "hi"}}]}).encode()
        ok_cm = MagicMock()
        ok_cm.read.return_value = ok_body
        ok_response = MagicMock()
        ok_response.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [self._http_error(429), ok_response]
        reply = call_model(
            [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "max",
            sleep_fn=lambda s: None,
        )
        self.assertEqual(reply, "hi")

    @patch("model_fix_loop.urllib.request.urlopen")
    def test_governor_is_acquired_per_attempt_and_reported(self, mock_urlopen):
        ok_body = json.dumps({"choices": [{"message": {"content": "hi"}}]}).encode()
        ok_cm = MagicMock()
        ok_cm.read.return_value = ok_body
        ok_response = MagicMock()
        ok_response.__enter__.return_value = ok_cm
        mock_urlopen.side_effect = [self._http_error(429), ok_response]
        with tempfile.TemporaryDirectory() as tmpdir:
            gov = Path(tmpdir) / "gov.json"
            # cooldown_seconds=0: the 429 must still be REPORTED (streak
            # increments, then the success resets it) without creating a
            # real-wall-clock cooldown this test would have to sit out --
            # cooldown waiting itself is covered by RateGovernorTests with
            # injected clocks.
            call_model(
                [{"role": "user", "content": "x"}], "https://u", "k", "m", 4096, "max",
                sleep_fn=lambda s: None, governor_path=gov,
                governor_cooldown_seconds=0, governor_max_cooldown_seconds=0,
            )
            state = json.loads(gov.read_text())
        # limited once (the 429) then reset by the success
        self.assertEqual(state["consecutive_limited"], 0)
        self.assertLess(state["tokens"], DEFAULT_GOVERNOR_BURST)  # slots were spent
```

(Import `DEFAULT_GOVERNOR_BURST` in the test import block. `governor_acquire` inside `call_model` reuses `call_model`'s `sleep_fn` — see Step 3.)

Add to the `_normalize_model_config` test class:

```python
    def test_governor_knobs_have_defaults(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["governor_calls_per_minute"], 30)
        self.assertEqual(config["governor_burst"], 5)
        self.assertEqual(config["governor_cooldown_seconds"], 30)
        self.assertEqual(config["governor_max_cooldown_seconds"], 300)
```

- [ ] **Step 2: Run to verify failure** — the new tests FAIL (429 currently raises; knobs missing).

- [ ] **Step 3: Implement**

1. `DEFAULT_RETRYABLE_HTTP_STATUSES = {429, 500, 502, 503, 504}` (update its comment: 429 is retryable now that the governor paces the fleet globally).
2. `call_model` signature append (after `prompt_cache="auto"`): `governor_path=None, governor_calls_per_minute=None, governor_burst=None, governor_cooldown_seconds=None, governor_max_cooldown_seconds=None`. `call_model` (~line 307) is defined BEFORE the Task-1 governor section (~line 610), so do NOT reference the `DEFAULT_GOVERNOR_*` names in the signature — resolve them at call time in the body: at the top of `call_model`, `governor_calls_per_minute = DEFAULT_GOVERNOR_CALLS_PER_MINUTE if governor_calls_per_minute is None else governor_calls_per_minute` (same pattern for the other three). Python resolves function-body names at call time, so the later definition is fine; a def-time default would raise NameError at import.
3. In the retry loop: immediately before the `_call_model_once` try, `governor_acquire(governor_path, governor_calls_per_minute, governor_burst, sleep_fn=sleep_fn)`. In the HTTPError handler, before `continue`/`raise`: `governor_report(governor_path, limited=(e.code in DEFAULT_RETRYABLE_HTTP_STATUSES), cooldown_seconds=governor_cooldown_seconds, max_cooldown_seconds=governor_max_cooldown_seconds)`. After a non-empty successful reply (just before `return reply`): `governor_report(governor_path, limited=False, cooldown_seconds=governor_cooldown_seconds, max_cooldown_seconds=governor_max_cooldown_seconds)`. URLError path: report `limited=False` (connection failures aren't rate limiting). Docstring: one paragraph on the governor kwargs.
4. `_normalize_model_config`: add the four knobs via `table.get(..., DEFAULT_...)`.
5. `main()`: find `make_logging_call_model` — inside its wrapper where `call_model(...)` is invoked, append `governor_path=DEFAULT_GOVERNOR_PATH, governor_calls_per_minute=cfg_for_phase["governor_calls_per_minute"], ...` for all four knobs, where `cfg_for_phase` is whatever config dict that closure already has in scope (inspect: the closures capture `config`/`review_config` — use the one in scope; if the closure is phase-generic, pass the worker config's knobs — the governor is account-global so worker-vs-reviewer knob differences don't matter; use the worker `config`).

- [ ] **Step 4: Run tests** — new tests PASS; `python3 -m unittest test_model_fix_loop` OK; discover OK.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: govern every model call; 429s become governed retries

call_model acquires a governor slot before every attempt and reports
every outcome; 429 joins the retryable set now that retries are paced
by the fleet-wide cooldown instead of per-process backoff.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 3: Differential target-tag verification (`tag_still_open` + recheck detail)

**Files:**
- Modify: `scripts/model_fix_loop.py` (new pure helper near `make_single_tag_gap` ~line 2263; `fix_gap`'s recheck gate; `main()`'s `recheck` closure ~line 2950)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- `tag_still_open(match, tag_gap) -> None | tuple` — `None` = closed; `("missing",)` = still missing; `("value_differs", exiftool_value, oxidex_value)` = present but wrong. `match` is a `group_gaps_by_format` entry (or `None` = everything closed).
- `fix_gap`'s `recheck_fn` contract widens: may return `int` (as today) **or** `(int, detail_str_or_None)`; when a detail string is present and the gap gate fails, the failure reason becomes that detail instead of the generic `"gap count did not decrease"`.

- [ ] **Step 1: Write the failing tests**

Add `tag_still_open,` to imports. New class:

```python
class TagStillOpenTests(unittest.TestCase):
    MISSING_GAP = {"format": "XMP", "kind": "missing", "tag_key": "XMP:XMP:ArtworkTitle",
                   "entry": {"family": "XMP", "name": "ArtworkTitle", "value": "test",
                             "tag_id": None, "source_file": None},
                   "parser_files": []}
    DIFF_GAP = {"format": "RW2", "kind": "diff", "tag_key": "RW2:EXIF:ISO",
                "entry": {"tag_key": "EXIF:ISO", "exiftool_value": "100",
                          "oxidex_value": "0", "source_file": None},
                "parser_files": []}

    def test_no_match_for_format_means_closed(self):
        self.assertIsNone(tag_still_open(None, self.MISSING_GAP))

    def test_still_missing(self):
        match = {"missing_tags": [{"family": "XMP", "name": "ArtworkTitle"}],
                 "value_differences": []}
        self.assertEqual(tag_still_open(match, self.MISSING_GAP), ("missing",))

    def test_missing_tag_that_arrived_with_wrong_value_is_STILL_OPEN(self):
        # The ArtworkTitle escape: leaves missing_in_oxidex, lands in
        # value_differences with the wrong value -- must NOT count closed.
        match = {"missing_tags": [],
                 "value_differences": [{"tag_key": "XMP:ArtworkTitle",
                                        "exiftool_value": "test",
                                        "oxidex_value": "test, verfänglich"}]}
        self.assertEqual(
            tag_still_open(match, self.MISSING_GAP),
            ("value_differs", "test", "test, verfänglich"),
        )

    def test_diff_tag_still_differing(self):
        match = {"missing_tags": [],
                 "value_differences": [{"tag_key": "EXIF:ISO",
                                        "exiftool_value": "100", "oxidex_value": "0"}]}
        self.assertEqual(tag_still_open(match, self.DIFF_GAP),
                         ("value_differs", "100", "0"))

    def test_fully_closed(self):
        match = {"missing_tags": [], "value_differences": []}
        self.assertIsNone(tag_still_open(match, self.MISSING_GAP))
        self.assertIsNone(tag_still_open(match, self.DIFF_GAP))


class FixGapRecheckDetailTests(unittest.TestCase):
    def test_tuple_recheck_detail_becomes_the_failure_reason(self):
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: (1, 'target still wrong: expected "test", got "test, x"'),
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            critique_fn=lambda *a, **k: "critique",
            log_fn=lambda s: None,
            max_repair_rounds=1,
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn('expected "test"', result["reason"])

    def test_plain_int_recheck_still_works(self):
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: 1,
            cargo_test_workspace_fn=lambda root: (True, ""),
            git_checkout_clean_fn=lambda root: None,
            critique_fn=lambda *a, **k: "critique",
            log_fn=lambda s: None,
            max_repair_rounds=1,
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("gap count did not decrease", result["reason"])
```

(`make_gap(gap_count=1)` exists; `fix_gap` fakes follow the file's existing patterns — match surrounding tests' kwargs if the exact fake set differs.)

- [ ] **Step 2: Run to verify failure** — ImportError on `tag_still_open`.

- [ ] **Step 3: Implement**

Above `make_single_tag_gap`:

```python
def tag_still_open(match, tag_gap):
    """Is this one tag still a gap in a fresh comparison? Checks BOTH
    lists regardless of the tag's original kind: a kind=="missing" tag
    that a fix made present-but-wrong moves from missing_in_oxidex into
    value_differences -- counting only its original list called that
    "closed", which is exactly how a wrong-valued XMP:ArtworkTitle fix
    passed recheck and survived to human sweep review. Returns None
    (closed), ("missing",), or ("value_differs", exiftool_value,
    oxidex_value) so the caller can put the actual values in front of
    the model on the retry."""
    if not match:
        return None
    if tag_gap["kind"] == "missing":
        fam, name = tag_gap["entry"]["family"], tag_gap["entry"]["name"]
        if any(t.get("family") == fam and t.get("name") == name
               for t in match.get("missing_tags") or []):
            return ("missing",)
        key = f"{fam}:{name}"
    else:
        key = tag_gap["entry"]["tag_key"]
    for d in match.get("value_differences") or []:
        if d.get("tag_key") == key:
            return ("value_differs", d.get("exiftool_value"), d.get("oxidex_value"))
    return None
```

`fix_gap`: replace the two lines

```python
        remaining = recheck_fn(fmt) if recheck_fn else gap["gap_count"]
```
with
```python
        recheck_result = recheck_fn(fmt) if recheck_fn else gap["gap_count"]
        recheck_detail = None
        if isinstance(recheck_result, tuple):
            remaining, recheck_detail = recheck_result
        else:
            remaining = recheck_result
```
and in the `remaining >= gap["gap_count"]` branch, `reason = recheck_detail or "gap count did not decrease"`. Add one docstring line to `recheck_fn`'s paragraph: it may return `(count, detail)` where detail replaces the generic failure reason.

`main()`'s `recheck` closure: after computing `match`, replace the kind-specific `present` logic with:

```python
            open_state = tag_still_open(match, tag_gap)
            if open_state and open_state[0] == "value_differs":
                return 1, (
                    f"target tag is present but its value is wrong -- "
                    f'expected (exiftool): "{open_state[1]}" / got (oxidex): "{open_state[2]}". '
                    "Fix the value, do not just emit the tag."
                )
            return 1 if open_state else 0
```

- [ ] **Step 4: Run tests** — new classes PASS; both suites OK.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "fix: differential target-tag verification closes the wrong-value escape

A missing tag 'fixed' with a wrong value moves lists instead of closing;
recheck now treats it as still open and feeds the exiftool-vs-oxidex
values into the retry as the failure reason.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 4: Sibling-tag clustering

**Files:**
- Modify: `scripts/model_fix_loop.py` (`cluster_key` + `make_cluster_gap` near `make_single_tag_gap`; `run_tag_loop` selection/bookkeeping ~line 2330; `fix_gap`'s `build_prompt` `max_tags`; `main()`'s `real_fix_tag` + recheck; `_normalize_model_config` `max_cluster_tags`)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- `cluster_key(tag_gap) -> tuple` — `(format, family, tuple(parser_files))`, family = middle component of `tag_key`.
- `make_cluster_gap(leader) -> gap dict` — unions leader + `leader.get("cluster_members", [])`; `gap_count` = member count; `"clustered": True`.
- `run_tag_loop(..., max_cluster_tags=1)` new kwarg (default 1 = today's behavior, so every existing test passes untouched); `main()` passes `cfg["max_cluster_tags"]`.
- Config: `max_cluster_tags = 6` default in `_normalize_model_config`; `DEFAULT_MAX_CLUSTER_TAGS = 6`.

- [ ] **Step 1: Write the failing tests**

Add `cluster_key, make_cluster_gap,` to imports. Tests:

```python
class ClusterKeyTests(unittest.TestCase):
    def test_family_is_the_middle_component(self):
        tg = {"format": "RW2", "tag_key": "RW2:EXIF:BlackLevelRed", "parser_files": ["a.rs"]}
        self.assertEqual(cluster_key(tg), ("RW2", "EXIF", ("a.rs",)))

    def test_different_parser_files_do_not_cluster(self):
        a = {"format": "F", "tag_key": "F:X:A", "parser_files": ["a.rs"]}
        b = {"format": "F", "tag_key": "F:X:B", "parser_files": ["b.rs"]}
        self.assertNotEqual(cluster_key(a), cluster_key(b))


class MakeClusterGapTests(unittest.TestCase):
    def _tg(self, name, kind="missing"):
        if kind == "missing":
            entry = {"family": "APP12", "name": name, "value": "1", "tag_id": None, "source_file": None}
        else:
            entry = {"tag_key": f"APP12:{name}", "exiftool_value": "1", "oxidex_value": "0", "source_file": None}
        return {"format": "JPEG", "tag_key": f"JPEG:APP12:{name}", "kind": kind,
                "entry": entry, "parser_files": ["j.rs"]}

    def test_leader_without_members_matches_single_tag_gap(self):
        leader = self._tg("MODE3")
        gap = make_cluster_gap(leader)
        self.assertEqual(gap["gap_count"], 1)
        self.assertTrue(gap["clustered"])
        self.assertEqual(len(gap["missing_tags"]), 1)

    def test_members_are_unioned_across_kinds(self):
        leader = self._tg("MODE3")
        leader["cluster_members"] = [self._tg("MODE4"), self._tg("MODE5", kind="diff")]
        gap = make_cluster_gap(leader)
        self.assertEqual(gap["gap_count"], 3)
        self.assertEqual(len(gap["missing_tags"]), 2)
        self.assertEqual(len(gap["value_differences"]), 1)
```

`RunTagLoop` clustering tests — model them on the file's existing `run_tag_loop` test fixtures (there is an established harness with fake `find_gaps_fn`/`fix_gap_fn`/state in a tempdir; REUSE its helpers rather than inventing new ones — find the class via `grep -n "class RunTagLoop" test_model_fix_loop.py`):

```python
    def test_clusters_siblings_onto_the_leader(self):
        seen = []
        def fake_fix(tag_gap, config, previous_attempts=None):
            seen.append([m["tag_key"] for m in [tag_gap] + tag_gap.get("cluster_members", [])])
            return {"status": "fixed", "gaps_closed": 1, "rounds": []}
        # three siblings same format/family/files + one outsider
        ...build tag_gaps via the class's existing gap-fixture helper with
        tag_keys JPEG:APP12:MODE3/MODE4/MODE5 and JPEG:COM:Other...
        run_tag_loop(..., fix_gap_fn=fake_fix, max_cluster_tags=6, max_rounds=1, ...)
        self.assertEqual(sorted(seen[0]), ["JPEG:APP12:MODE3", "JPEG:APP12:MODE4", "JPEG:APP12:MODE5"])

    def test_fixed_clears_state_for_every_member(self): ...state file has no MODE3/4/5 keys after...
    def test_failure_charges_only_the_leader(self): ...MODE3 fails=1; MODE4/5 have no fails/claim...
    def test_max_cluster_tags_1_disables_clustering(self): ...cluster_members absent...
```

(The implementer writes these four concretely against the real fixture helpers — the assertions above are the required behavior; "..." marks fixture plumbing to copy from neighboring tests, NOT optional work.)

- [ ] **Step 2: Run to verify failure** — ImportError on `cluster_key`.

- [ ] **Step 3: Implement**

```python
DEFAULT_MAX_CLUSTER_TAGS = 6


def cluster_key(tag_gap):
    """Which tags belong in one conversation: same format, same family
    (middle component of the tag key -- EXIF, APP12, ZIP, ...), same
    parser files. Sibling tags in one table are usually one generalized
    branch away from each other (BlackLevelRed/Green/Blue, MODE1..6,
    ZipCRC/ZipCompressedSize all were), so investigating them separately
    re-pays the whole context cost per tag."""
    parts = tag_gap["tag_key"].split(":")
    family = parts[1] if len(parts) >= 3 else ""
    return (tag_gap["format"], family, tuple(tag_gap.get("parser_files") or ()))


def make_cluster_gap(leader):
    """make_single_tag_gap generalized to a leader plus its
    cluster_members: one gap dict whose tag lists union every member,
    gap_count == member count (so fix_gap's decrease check means "at
    least one of these closed"), and "clustered": True (so build_prompt
    shows every member even when max_prompt_tags is 1)."""
    members = [leader] + list(leader.get("cluster_members") or [])
    missing = [m["entry"] for m in members if m["kind"] == "missing"]
    diffs = [m["entry"] for m in members if m["kind"] == "diff"]
    return {
        "format": leader["format"],
        "missing_tags": missing,
        "value_differences": diffs,
        "gap_count": len(members),
        "parser_files": leader["parser_files"],
        "clustered": True,
    }
```

`run_tag_loop`: add kwarg `max_cluster_tags=1`. After `tag_gap = active[0]` and its claim, add:

```python
        if max_cluster_tags > 1:
            leader_key = cluster_key(tag_gap)
            members = []
            for cand in active[1:]:
                if len(members) >= max_cluster_tags - 1:
                    break
                if cluster_key(cand) == cluster_key(tag_gap):
                    members.append(cand)
            for m in members:
                m_entry = state.setdefault(m["tag_key"], {"fails": 0, "blacklisted": False, "attempts": []})
                m_entry["claimed_by"] = worker_id
                m_entry["claimed_at"] = time.time()
                seen_tag_keys.add(m["tag_key"])
            if members:
                tag_gap = dict(tag_gap, cluster_members=members)
                save_state_fn(state_path, state)
                log_fn(f"clustered {len(members)} sibling tag(s) with {tag_gap['tag_key']}")
```

Bookkeeping after `result = fix_gap_fn(...)`: where status "fixed"/"duplicate" pops the leader's state, also `for m in tag_gap.get("cluster_members") or []: state.pop(m["tag_key"], None)`. In the failure branch, release members without charging: `for m in tag_gap.get("cluster_members") or []: me = state.get(m["tag_key"]); me and (me.pop("claimed_by", None), me.pop("claimed_at", None))` (write it as a normal loop, not a tuple trick).

`fix_gap`'s `build_prompt` call: `max_tags=` becomes
```python
        max_tags=(len(gap["missing_tags"]) + len(gap["value_differences"]))
                 if gap.get("clustered") else config["max_prompt_tags"],
```

`main()`: `real_fix_tag` builds `single_gap = make_cluster_gap(tag_gap)` instead of `make_single_tag_gap(tag_gap)` (make_cluster_gap with no members ≡ single, keeping one code path — verify `make_single_tag_gap` remains for its existing tests/callers). The `prompt_preview` `build_prompt` call in `real_fix_tag` must mirror `fix_gap`'s clustered `max_tags` (pass `max_tags=single_gap["gap_count"] if single_gap.get("clustered") else cfg["max_prompt_tags"]`) so the preview shows what the model actually receives. The `recheck` closure counts ALL targets:

```python
            targets = [tag_gap] + list(tag_gap.get("cluster_members") or [])
            open_count, detail = 0, None
            for t in targets:
                st = tag_still_open(match, t)
                if st:
                    open_count += 1
                    if st[0] == "value_differs" and detail is None:
                        detail = (f'{t["tag_key"]}: present but wrong -- expected (exiftool): '
                                  f'"{st[1]}" / got (oxidex): "{st[2]}". Fix the value.')
            return (open_count, detail) if detail else open_count
```

`run_tag_loop` call site in `main()`: pass `max_cluster_tags=cfg["max_cluster_tags"]`. `_normalize_model_config`: `"max_cluster_tags": table.get("max_cluster_tags", DEFAULT_MAX_CLUSTER_TAGS),`.

- [ ] **Step 4: Run tests** — all new PASS; both suites OK (existing run_tag_loop tests unaffected because the default is 1... note `main()` passes 6 via config but tests drive `run_tag_loop` directly).

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: cluster sibling tags into one fix conversation

Same-format/family/parser-file tags ride one investigation (leader +
cluster_members): one prompt covers the family, a fix clears every
member's state, and a failure charges only the leader.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 5: Neighbor-precedent block

**Files:**
- Modify: `scripts/model_fix_loop.py` (new builder near `build_perl_reference_block`; `build_prompt` kwarg + assembly; `fix_gap` pass-through; `main()` threading)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- `find_implemented_sibling(gap, repo_root) -> str | None` — pure-ish (reads files): first `"<family>:<Name>"` literal in the gap's parser files whose Name isn't one of the gap's own tags.
- `build_neighbor_precedent_block(gap, repo_root, git_runner_fn=None) -> str` — `""` on any miss. `git_runner_fn(args_list, cwd) -> str` injectable (default wraps `subprocess.run(["git", ...], ...)` with `# nosec B603` and a 10s timeout); truncate the shown patch to `DEFAULT_MAX_PRECEDENT_CHARS = 3000`.
- `build_prompt(..., neighbor_precedent_block="")` — pre-rendered string kwarg (built by the caller, keeping build_prompt free of subprocess), inserted after `perl_block` in the stable per-tag section.

- [ ] **Step 1: Write the failing tests**

```python
class NeighborPrecedentTests(unittest.TestCase):
    def _gap(self, tmp):
        (tmp / "j.rs").write_text('metadata.insert("APP12:ColorMode".to_string(), v);')
        return {"format": "JPEG",
                "missing_tags": [{"family": "APP12", "name": "MODE3", "value": "0",
                                  "tag_id": None, "source_file": None}],
                "value_differences": [], "gap_count": 1, "parser_files": ["j.rs"]}

    def test_finds_an_implemented_sibling_literal(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            gap = self._gap(tmp)
            self.assertEqual(find_implemented_sibling(gap, tmp), "APP12:ColorMode")

    def test_own_gap_tags_are_not_their_own_precedent(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            (tmp / "j.rs").write_text('metadata.insert("APP12:MODE3".to_string(), v);')
            gap = self._gap(tmp)
            self.assertIsNone(find_implemented_sibling(gap, tmp))

    def test_block_includes_the_historic_patch(self):
        calls = []
        def fake_git(args, cwd):
            calls.append(args)
            if args[0] == "log":
                return "abc123\n"
            return "commit abc123\n+++ test added here\n"
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            block = build_neighbor_precedent_block(self._gap(tmp), tmp, git_runner_fn=fake_git)
        self.assertIn("APP12:ColorMode", block)
        self.assertIn("test added here", block)
        self.assertIn("-S", str(calls[0]))

    def test_git_failure_yields_empty_block(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            tmp = Path(tmpdir)
            block = build_neighbor_precedent_block(
                self._gap(tmp), tmp, git_runner_fn=lambda a, c: "")
        self.assertEqual(block, "")


class BuildPromptNeighborPrecedentTests(unittest.TestCase):
    def test_block_appears_in_the_stable_section(self):
        gap = make_gap(gap_count=1)
        prompt = build_prompt(gap, neighbor_precedent_block="\n\nPRECEDENT-MARKER-XYZ")
        self.assertIn("PRECEDENT-MARKER-XYZ", prompt)
        self.assertLess(prompt.index("PRECEDENT-MARKER-XYZ"),
                        prompt.index("Previous attempts") if "Previous attempts" in prompt
                        else len(prompt))
```

- [ ] **Step 2: Run to verify failure** — ImportError.

- [ ] **Step 3: Implement**

```python
DEFAULT_MAX_PRECEDENT_CHARS = 3000
SIBLING_LITERAL_RE_TEMPLATE = r'"({family}:[A-Za-z0-9_]+)"'


def find_implemented_sibling(gap, repo_root):
    """First already-implemented same-family tag literal in the gap's own
    parser files, excluding the gap's own tags -- the nearest working
    example of 'how tags in this table get wired'."""
    families, own = set(), set()
    for e in gap["missing_tags"]:
        families.add(e["family"]); own.add(f'{e["family"]}:{e["name"]}')
    for d in gap["value_differences"]:
        fam = d["tag_key"].split(":")[0]
        families.add(fam); own.add(d["tag_key"])
    for f in gap["parser_files"]:
        try:
            text = (Path(repo_root) / f).read_text(errors="replace")
        except OSError:
            continue
        for family in sorted(families):
            for m in re.finditer(SIBLING_LITERAL_RE_TEMPLATE.format(family=re.escape(family)), text):
                if m.group(1) not in own:
                    return m.group(1)
    return None


def _run_git(args, cwd):
    try:
        result = subprocess.run(  # nosec B603 -- fixed git argv, no untrusted input
            ["git", *args], capture_output=True, text=True, cwd=cwd, timeout=10,
        )
    except (OSError, subprocess.SubprocessError):
        return ""
    return result.stdout if result.returncode == 0 else ""


def build_neighbor_precedent_block(gap, repo_root, git_runner_fn=None):
    """The historical diff that added the nearest implemented sibling tag
    -- nearly every landed fix this loop has produced was 'generalize the
    neighboring tag's branch' (LightS from ColorMode, MODE3-6 from
    MODE1/2, ZipCRC from ZipBitFlag), so showing how the neighbor was
    added (implementation AND its test) up front replaces a whole
    REQUEST investigation. Empty string on any miss; never fatal."""
    git_runner_fn = git_runner_fn or _run_git
    sibling = find_implemented_sibling(gap, repo_root)
    if not sibling:
        return ""
    files = list(gap["parser_files"])
    sha = git_runner_fn(["log", "-S", sibling, "-1", "--format=%H", "--", *files], repo_root).strip()
    if not sha:
        return ""
    patch = git_runner_fn(["show", sha], repo_root)
    if not patch:
        return ""
    if len(patch) > DEFAULT_MAX_PRECEDENT_CHARS:
        patch = patch[:DEFAULT_MAX_PRECEDENT_CHARS] + "\n... (truncated)"
    return (
        f"\n\nHow the neighboring tag {sibling} was added to this same code path "
        "(historical commit -- pattern-match this, including adding an equivalent test):\n"
        f"{patch}"
    )
```

`build_prompt`: kwarg `neighbor_precedent_block=""` + docstring line; insert `f"{neighbor_precedent_block}"` right after `f"{perl_block}"` in the assembly. `fix_gap`: kwarg `neighbor_precedent_block=""` passed through to its `build_prompt` call. `main()` `real_fix_tag`: compute once per attempt `precedent = build_neighbor_precedent_block(single_gap, REPO_ROOT)` and pass to both the `prompt_preview` `build_prompt` call and `fix_gap`.

- [ ] **Step 4: Run tests** — both suites OK.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat: neighbor-precedent block -- show how the sibling tag was added

Finds the nearest implemented same-family tag literal in the gap's own
parser files and inlines the historical commit that added it (impl +
test), replacing a whole REQUEST investigation with a working example.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 6: Cheaper rounds — targeted tests first, full suite pre-commit, sccache

**Files:**
- Modify: `scripts/model_fix_loop.py` (`cargo_test_targeted` next to `cargo_test_workspace` ~line 500; `cargo_env` ~line 606; `fix_gap` ordering; `main()` sets `OXIDEX_USE_SCCACHE`; `_normalize_model_config` `use_sccache`)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- `cargo_test_targeted(repo_root, filter_str) -> (success, output)` — `cargo test --lib <filter_str>`; zero matching tests is cargo-success.
- `fix_gap(..., cargo_test_targeted_fn=cargo_test_targeted)` — new injectable; existing `**kwargs` fakes absorb it.
- New `fix_gap` ordering: build → gap gate → **targeted tests** → duplicate → review → **full workspace suite** → commit. Full-suite failure post-review reverts and is a `test_regressed` round exactly like today's.
- `cargo_env`: adds `RUSTC_WRAPPER=sccache` iff sccache on PATH, `RUSTC_WRAPPER` unset, and `os.environ.get("OXIDEX_USE_SCCACHE") != "0"`. `main()` sets `OXIDEX_USE_SCCACHE` from `cfg["use_sccache"]` (`"1"`/`"0"`) before its first cargo use. `_normalize_model_config`: `"use_sccache": table.get("use_sccache", True)`.

- [ ] **Step 1: Write the failing tests**

```python
class CargoTestTargetedTests(unittest.TestCase):
    @patch("model_fix_loop.subprocess.run")
    def test_runs_lib_tests_with_the_filter(self, mock_run):
        mock_run.return_value = MagicMock(returncode=0, stdout="ok\n", stderr="")
        ok, output = cargo_test_targeted(Path("/fake"), "app12")
        self.assertTrue(ok)
        self.assertEqual(mock_run.call_args[0][0], ["cargo", "test", "--lib", "app12"])


class CargoEnvSccacheTests(unittest.TestCase):
    @patch("model_fix_loop.shutil.which")
    def test_sets_wrapper_when_available_and_enabled(self, mock_which):
        mock_which.return_value = "/opt/homebrew/bin/sccache"
        with patch.dict(os.environ, {"OXIDEX_USE_SCCACHE": "1"}, clear=False):
            os.environ.pop("RUSTC_WRAPPER", None)
            env = cargo_env()
        self.assertEqual(env.get("RUSTC_WRAPPER"), "sccache")

    @patch("model_fix_loop.shutil.which")
    def test_disabled_by_env_flag(self, mock_which):
        mock_which.return_value = "/opt/homebrew/bin/sccache"
        with patch.dict(os.environ, {"OXIDEX_USE_SCCACHE": "0"}, clear=False):
            env = cargo_env()
        self.assertNotEqual(env.get("RUSTC_WRAPPER"), "sccache")


class FixGapTestOrderingTests(unittest.TestCase):
    def test_targeted_runs_before_review_full_suite_only_before_commit(self):
        order = []
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: 0,
            cargo_test_targeted_fn=lambda root, f: (order.append("targeted"), (True, ""))[1],
            cargo_test_workspace_fn=lambda root: (order.append("full"), (True, ""))[1],
            review_fn=lambda *a, **k: (order.append("review"), (True, ""))[1],
            git_commit_fn=lambda msg, root: order.append("commit"),
            git_checkout_clean_fn=lambda root: None,
            detect_duplicate_fn=lambda *a: False,
            log_fn=lambda s: None,
        )
        self.assertEqual(result["status"], "fixed")
        self.assertEqual(order, ["targeted", "review", "full", "commit"])

    def test_targeted_failure_is_a_test_regressed_round_without_full_suite(self):
        full_runs = []
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: 0,
            cargo_test_targeted_fn=lambda root, f: (False, "targeted boom"),
            cargo_test_workspace_fn=lambda root: (full_runs.append(1), (True, ""))[1],
            git_checkout_clean_fn=lambda root: None,
            critique_fn=lambda *a, **k: "critique",
            log_fn=lambda s: None,
            max_repair_rounds=1,
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("targeted boom", result["reason"])
        self.assertEqual(full_runs, [])

    def test_full_suite_failure_before_commit_reverts_and_fails_the_round(self):
        commits = []
        result = fix_gap(
            make_gap(gap_count=1), CONFIG,
            attempt_build_fn=lambda messages, **kwargs: (True, None, "--- a/x\n+++ b/x\n", messages),
            recheck_fn=lambda fmt: 0,
            cargo_test_targeted_fn=lambda root, f: (True, ""),
            cargo_test_workspace_fn=lambda root: (False, "full boom"),
            review_fn=lambda *a, **k: (True, ""),
            git_commit_fn=lambda msg, root: commits.append(1),
            git_checkout_clean_fn=lambda root: None,
            detect_duplicate_fn=lambda *a: False,
            critique_fn=lambda *a, **k: "critique",
            log_fn=lambda s: None,
            max_repair_rounds=1,
        )
        self.assertEqual(result["status"], "failed")
        self.assertIn("full boom", result["reason"])
        self.assertEqual(commits, [])
```

(`review_fn` signature: match the file's existing fakes. `os` is already imported in the test file — verify, add if missing.)

- [ ] **Step 2: Run to verify failure** — ImportError on `cargo_test_targeted`.

- [ ] **Step 3: Implement**

```python
def cargo_test_targeted(repo_root, filter_str):
    """Fast first-line test gate: only lib tests whose names match
    filter_str (the format lowercased -- best-effort, zero matches is a
    pass, which cargo already treats as success). The full workspace
    suite still gates every commit; this just stops candidates that are
    about to die at review from paying the full-suite price first."""
    result = subprocess.run(  # nosec B603
        ["cargo", "test", "--lib", filter_str],
        capture_output=True, text=True, cwd=repo_root, env=cargo_env(),
    )
    return result.returncode == 0, result.stdout + result.stderr
```

`cargo_env()`: after its existing env copy, add:

```python
    if (
        os.environ.get("OXIDEX_USE_SCCACHE") != "0"
        and "RUSTC_WRAPPER" not in env
        and shutil.which("sccache")
    ):
        env["RUSTC_WRAPPER"] = "sccache"
```

(match the function's actual local variable name for the env dict.)

`fix_gap`: signature gains `cargo_test_targeted_fn=cargo_test_targeted` (next to `cargo_test_workspace_fn`) + docstring line. Restructure the post-gap-gate section to:

1. targeted: `t_ok, t_out = cargo_test_targeted_fn(repo_root, fmt.lower())`; on failure → revert, reason `f"targeted tests ({fmt.lower()}) regressed:\n{t_out[-DEFAULT_MAX_TEST_OUTPUT_CHARS:]}"`, `critique_and_continue("test_regressed", ...)`.
2. duplicate check (unchanged, moves up).
3. review (unchanged position relative to duplicate).
4. on approval, BEFORE `git_commit_fn`: `tests_passed, test_output = cargo_test_workspace_fn(repo_root)`; on failure → revert, reason `f"cargo test --workspace regressed:\n{tail}"`, `critique_and_continue("test_regressed", ...)` (continue the round loop); else commit + return fixed.

Existing tests to repair (semantics preserved): any test asserting the OLD order (full suite before review) — find with `grep -n "cargo_test_workspace_fn" test_model_fix_loop.py` and re-read each; most pass fakes returning success for both and won't notice. `test_fails_when_tests_regress`-style tests now need `cargo_test_targeted_fn=lambda root, f: (True, "")` (or their failure moves to the pre-commit position) — update minimally, keep each test's original intent, note every change in your report.

`main()`: right after `cfg` is built, `os.environ["OXIDEX_USE_SCCACHE"] = "1" if cfg.get("use_sccache", True) else "0"`. `_normalize_model_config`: add `"use_sccache": table.get("use_sccache", True),`.

- [ ] **Step 4: Run tests** — both suites OK.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "perf: targeted tests gate candidates; full suite only before commit; sccache

cargo test --lib <format> runs in seconds and catches the common
regressions; the full workspace suite still gates every commit.
cargo_env opportunistically enables sccache across the 21 near-identical
worktrees (use_sccache=false / OXIDEX_USE_SCCACHE=0 to disable).

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 7: Landed-tags skip set

**Files:**
- Modify: `scripts/model_fix_loop.py` (`load_landed_tags` near `load_tag_state`; `run_tag_loop` skip; `main()` threading), `scripts/log_sweep_review.py` (`--landed-log` append on accepted)
- Test: `scripts/test_model_fix_loop.py`, `scripts/test_log_sweep_review.py`

**Interfaces:**
- `DEFAULT_LANDED_TAGS_PATH = OXIDEX_HOME / "logs" / "landed-tags.log"`; lines are `<iso-ts> <tag_key>`.
- `load_landed_tags(path) -> set[str]` — missing/corrupt file → empty set.
- `run_tag_loop(..., landed_tags_path=None)` — when set, re-read each round; active entries whose `tag_key` is in the set are skipped (state cleared like a duplicate, logged `"[X] skipped -- already landed via sweep"`).
- `log_sweep_review.append_sweep_review(..., landed_log_path=None)` — on `verdict == "accepted"`, also append `<iso-ts> <format>:<tag>` to `landed_log_path`; CLI flag `--landed-log` defaulting to the path above.

- [ ] **Step 1: Write the failing tests**

`test_model_fix_loop.py`:

```python
class LoadLandedTagsTests(unittest.TestCase):
    def test_missing_file_is_empty_set(self):
        self.assertEqual(load_landed_tags(Path("/nonexistent/landed.log")), set())

    def test_parses_tag_keys_skipping_malformed_lines(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            p = Path(tmpdir) / "landed.log"
            p.write_text("2026-07-23T17:00:00 JPEG:APP12:MODE3\n\ngarbage-no-space\n"
                         "2026-07-23T17:05:00 PSD:EXIF:Compression\n")
            self.assertEqual(load_landed_tags(p),
                             {"JPEG:APP12:MODE3", "PSD:EXIF:Compression"})
```

Plus one `run_tag_loop` test in its existing fixture class: two active tags, one pre-written into a landed file → `fix_gap_fn` sees only the other; the landed tag's state entry is popped; the log contains `"already landed via sweep"`.

`test_log_sweep_review.py` (mirror its existing style):

```python
    def test_accepted_verdict_appends_to_landed_log(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "reviews.jsonl"
            landed = Path(tmpdir) / "landed.log"
            append_sweep_review(log, "JPEG", "APP12:MODE3", "accepted", "verified",
                                landed_log_path=landed, now_fn=lambda: 1_784_800_000)
            self.assertIn("JPEG:APP12:MODE3", landed.read_text())

    def test_rejected_verdict_does_not_touch_landed_log(self):
        with tempfile.TemporaryDirectory() as tmpdir:
            log = Path(tmpdir) / "reviews.jsonl"
            landed = Path(tmpdir) / "landed.log"
            append_sweep_review(log, "XMP", "XMP:ArtworkTitle", "rejected", "wrong value",
                                landed_log_path=landed, now_fn=lambda: 1_784_800_000)
            self.assertFalse(landed.exists())
```

- [ ] **Step 2: Run to verify failure** — ImportErrors in both files.

- [ ] **Step 3: Implement**

`model_fix_loop.py` (near `load_tag_state`):

```python
DEFAULT_LANDED_TAGS_PATH = OXIDEX_HOME / "logs" / "landed-tags.log"


def load_landed_tags(path):
    """tag_keys the sweep has already landed (see log_sweep_review.py's
    accepted-verdict append) -- workers skip these instead of re-deriving
    a fix that's already merged (observed live: the ZIP worker reproduced
    the identical ZipCRC diff a full round after the sweep landed it).
    Missing/corrupt file = empty set; each line is "<iso-ts> <tag_key>"."""
    try:
        text = Path(path).read_text()
    except OSError:
        return set()
    landed = set()
    for line in text.splitlines():
        parts = line.strip().split(None, 1)
        if len(parts) == 2:
            landed.add(parts[1])
    return landed
```

`run_tag_loop`: kwarg `landed_tags_path=None` + docstring line. At the top of each round (right after `state = load_state_fn(state_path)` where `active` is computed), load `landed = load_landed_tags(landed_tags_path) if landed_tags_path else set()`; filter: before selection, for each candidate in `active` whose `tag_key` is in `landed`: `log_fn(f"[{tg['tag_key']}] skipped -- already landed via sweep")`, `state.pop(tg["tag_key"], None)`, remove from `active`; `save_state_fn` once if anything was popped. `main()` passes `landed_tags_path=DEFAULT_LANDED_TAGS_PATH` at its `run_tag_loop` call.

`log_sweep_review.py`: `append_sweep_review(..., landed_log_path=None)`; after writing the JSONL line, `if verdict == "accepted" and landed_log_path:` append `f"{entry['timestamp']} {format_name}:{tag}\n"` (create parents). `main()`: `--landed-log` arg (default `DEFAULT_LANDED_TAGS_PATH` equivalent — define its own `OXIDEX_HOME`-based default matching the module's existing style) passed through.

- [ ] **Step 4: Run tests** — `python3 -m unittest test_log_sweep_review test_model_fix_loop` OK; discover OK.

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py scripts/log_sweep_review.py scripts/test_log_sweep_review.py
git commit -m "feat: landed-tags skip set -- sweep acceptances stop worker re-derivation

log_sweep_review --verdict accepted now also appends the tag to
landed-tags.log; run_tag_loop re-reads it every round and skips (and
un-claims) tags the sweep already merged.

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

### Task 8: Config defaults, docstring, final gate

**Files:**
- Modify: `scripts/model_fix_loop.py` (module docstring config table), `config.example.toml`, live `config.toml` (gitignored — edit, do not `git add`)
- Test: `scripts/test_model_fix_loop.py` (one aggregated defaults test)

- [ ] **Step 1: Write the failing test**

```python
    def test_throughput_knobs_have_defaults(self):
        config = _normalize_model_config({"base_url": "u", "api_key": "k", "models": ["m"]})
        self.assertEqual(config["max_cluster_tags"], 6)
        self.assertEqual(config["use_sccache"], True)
        self.assertEqual(config["governor_calls_per_minute"], 30)
```

(If Tasks 2/4/6 already added the keys this passes immediately — it then serves as the aggregated regression lock; that is fine, note it.)

- [ ] **Step 2: Run** — record pass/fail.

- [ ] **Step 3: Implement**

Module docstring config table: add entries for `governor_calls_per_minute/burst/cooldown_seconds/max_cooldown_seconds`, `max_cluster_tags`, `use_sccache` in the established style. `config.example.toml` under `[worker]` after `compaction_keep_recent_turns`:

```toml
governor_calls_per_minute = 30
governor_burst = 5
governor_cooldown_seconds = 30
governor_max_cooldown_seconds = 300
max_cluster_tags = 6
use_sccache = true
```

Live `config.toml`: same six lines in `[worker]`. Sanity-load:
`python3 -c "import model_fix_loop as m; d=m.load_toml_config(m.DEFAULT_CONFIG_PATH); c=m._normalize_model_config(d['worker']); print(c['governor_calls_per_minute'], c['max_cluster_tags'], c['use_sccache'])"` → `30 6 True`.

- [ ] **Step 4: Run the full gates** — `python3 -m unittest discover -p "test_*.py"` OK (466 baseline + all new).

- [ ] **Step 5: Commit**

```bash
cd /Users/allen/.oxidex/worktrees/sweep-tags
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py config.example.toml
git commit -m "feat: config defaults + docs for the throughput features

Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>"
```

---

## Final integration (orchestrator, not a task subagent)

1. Full discover suite once more; push `feat/model-fix-loop-context` → PR #41 + summary comment.
2. Merge into local `main`; suite there.
3. Propagate `model_fix_loop.py`, `log_sweep_review.py`, `config.toml` to every `~/.oxidex/worktrees/parallel-fix/model-fix-*`.
4. Seed `landed-tags.log` with today's sweep-landed tags (PSD:EXIF:Compression family, JPEG:APP12:MODE3-6, CR3 CMT1 Artist — exact keys from the sweep commits).
5. Remind: dispatcher restart required; suggest `--max-parallel 10` is now safe to try because of the governor.
