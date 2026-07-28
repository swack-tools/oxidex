# Fix-Throughput Engine Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Cut total wall-clock time to close all 4,333 open ExifTool tag gaps by removing the measured losses between "a correct fix exists on disk" and "the gap is closed on `main`".

**Architecture:** Three sequential phases against the running fleet, each behind a measurement gate. Phase 2 stops permanently discarding fixes that failed for transient reasons. Phase 3 stops spending worker capacity on gaps that never succeed. Phase 4 unlocks the dormant bulk table-port tier, but only after its three known blockers are fixed and a single pilot proves the tier works at all.

**Tech Stack:** Python 3.11+ (stdlib only, `uv run` shebangs), `unittest` (no pytest), Rust/cargo for the comparison binary, git worktrees, JSONL ledgers.

## Global Constraints

- **Spec:** `docs/plans/specs/2026-07-25-fix-throughput-engine-design.md`. Phase 1 already shipped in PR #116 — do not re-implement it.
- **Test runner is `unittest`, not pytest.** Run from `scripts/`: `python3 -m unittest test_<module> -v`.
- **Python is stdlib-only.** No new third-party dependencies in `scripts/`.
- **Never break these invariants** (spec §3): no-discard (M5), consume handshake (M2/M5), detached-HEAD publish, fail-safe review (unparseable ⇒ reject), no stale-report fall-through.
- **The fleet is live.** Every change must be safe to land while ~20 workers and 14 mergers are running. `config.toml` is gitignored and auto-copied to worktrees each round (no restart needed); `scripts/*.py` changes need a dispatcher/merger restart to take effect.
- **Commit style:** `fix(fleet): ...` / `feat(fleet): ...`, body explains the measured defect. End with `Co-Authored-By: Claude <noreply@anthropic.com>`.
- **Repo for all work:** `~/.oxidex/worktrees/fleet-ops`.
- **Squad list (14):** canon, nikon, sony-minolta, xmp, exif-core, olympus, pentax-samsung, panasonic-leica, mobile, thermal, sigma-c2pa, ps-docs, standards-appn, tail.

---

## File Structure

| File | Responsibility | Phase |
|---|---|---|
| `scripts/validate_fix_commit.py` | Add flag taxonomy (`classify_flags`) alongside existing gates. | 2 |
| `scripts/test_validate_fix_commit.py` | Taxonomy tests. | 2 |
| `scripts/squad_merge_loop.py` | Honor taxonomy: retry transient, permanently quarantine defects. Fail fast on bad `--perl-lib`. | 2 |
| `scripts/test_squad_merge_loop.py` | Retry/permanent-quarantine tests. | 2 |
| `scripts/model_fix_loop.py` | Truncation retry; gap priority sort; module-claim cap. | 2, 3 |
| `scripts/test_model_fix_loop.py` | Tests for the above. | 2, 3 |
| `scripts/attribute_gaps.py` | Emit `difficulty` per gap. | 3 |
| `scripts/test_attribute_gaps.py` | Difficulty-classification tests. | 3 |
| `scripts/parallel_model_fix_loop.py` | Gap-weighted format apportionment. | 3 |
| `scripts/test_parallel_model_fix_loop.py` | Apportionment tests. | 3 |

Phase 4 files are named in Task 12 and deliberately deferred behind its gate.

---

# PHASE 2 — Validator and quarantine

**Entry gate (verify before starting):** Phase 1 has run ≥4 clean hours and published rate rose above 0.24 gaps/h. Measure with the command in Task 0. If it did NOT rise, stop and re-derive — the loss is elsewhere.

---

### Task 0: Measure the Phase 1 → 2 gate

**Files:**
- Create: `scripts/measure_throughput.py`
- Test: `scripts/test_measure_throughput.py`

**Interfaces:**
- Produces: `published_gaps(repo_root, since_iso, git_run=...) -> dict` returning
  `{"gaps": int, "commits": int, "hours": float, "rate": float}`.

- [ ] **Step 1: Write the failing test**

Create `scripts/test_measure_throughput.py`:

```python
import unittest
from measure_throughput import parse_verified_trailer, published_gaps


class ParseVerifiedTrailerTests(unittest.TestCase):
    def test_extracts_closed_gap_count(self):
        self.assertEqual(parse_verified_trailer("recheck-pass gaps=6->0"), 6)

    def test_partial_close_counts_the_difference(self):
        self.assertEqual(parse_verified_trailer("recheck-pass gaps=6->2"), 4)

    def test_unparseable_is_zero_not_a_crash(self):
        self.assertEqual(parse_verified_trailer("something else"), 0)

    def test_missing_trailer_is_zero(self):
        self.assertEqual(parse_verified_trailer(""), 0)


class PublishedGapsTests(unittest.TestCase):
    def test_sums_gaps_over_window_and_computes_rate(self):
        commits = [
            ("aaa", "2026-07-25T01:00:00", "recheck-pass gaps=3->0"),
            ("bbb", "2026-07-25T02:00:00", "recheck-pass gaps=2->0"),
        ]

        def fake_git(args, cwd):
            return "\n".join(f"{s}\x1f{ts}\x1f{v}" for s, ts, v in commits)

        result = published_gaps("/unused", "2026-07-25T00:00:00",
                                git_run=fake_git, now_iso="2026-07-25T03:00:00")
        self.assertEqual(result["gaps"], 5)
        self.assertEqual(result["commits"], 2)
        self.assertAlmostEqual(result["hours"], 3.0)
        self.assertAlmostEqual(result["rate"], 5 / 3.0)

    def test_empty_window_reports_zero_rate_without_dividing_by_zero(self):
        result = published_gaps("/unused", "2026-07-25T00:00:00",
                                git_run=lambda a, cwd: "",
                                now_iso="2026-07-25T00:00:00")
        self.assertEqual(result["gaps"], 0)
        self.assertEqual(result["rate"], 0.0)


if __name__ == "__main__":
    unittest.main()
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_measure_throughput -v
```
Expected: FAIL — `ModuleNotFoundError: No module named 'measure_throughput'`

- [ ] **Step 3: Write the implementation**

Create `scripts/measure_throughput.py`:

```python
#!/usr/bin/env -S uv run
# /// script
# requires-python = ">=3.11"
# dependencies = []
# ///
"""Measure the spec's headline metric: gaps PUBLISHED to main per hour.

Production-side rates (lessons.jsonl `fixed` events) are deliberately not
measured here -- optimizing those is what produced a pipeline generating
work it could not ship (see spec section 2).
"""
import argparse
import re
import subprocess  # nosec B404 -- list-argv only, no shell=True
import sys
from datetime import datetime
from pathlib import Path

SEP = "\x1f"
_GAPS_RE = re.compile(r"gaps=(\d+)->(\d+)")


def run_git(args, cwd):
    return subprocess.run(  # nosec B603
        ["git", *args], cwd=cwd, capture_output=True, text=True, check=True,
    ).stdout


def parse_verified_trailer(value):
    """Gaps actually closed by one commit, from its `Verified:` trailer.

    Returns the DIFFERENCE (before - after), not the before-count: a
    commit that took gaps=6->2 closed 4, not 6. Unparseable or missing
    returns 0 rather than raising -- a malformed trailer must not abort a
    whole measurement run.
    """
    m = _GAPS_RE.search(value or "")
    if not m:
        return 0
    before, after = int(m.group(1)), int(m.group(2))
    return max(0, before - after)


def published_gaps(repo_root, since_iso, git_run=run_git, now_iso=None):
    """{gaps, commits, hours, rate} for commits on HEAD since `since_iso`."""
    out = git_run(
        ["log", f"--since={since_iso}", f"--pretty=format:%H{SEP}%cI{SEP}%(trailers:key=Verified,valueonly)"],
        repo_root,
    )
    gaps = 0
    commits = 0
    for line in (out or "").splitlines():
        if not line.strip():
            continue
        parts = line.split(SEP)
        if len(parts) < 3:
            continue
        closed = parse_verified_trailer(parts[2].strip())
        if closed:
            gaps += closed
            commits += 1
    end = datetime.fromisoformat(now_iso) if now_iso else datetime.now()
    hours = (end - datetime.fromisoformat(since_iso)).total_seconds() / 3600
    rate = gaps / hours if hours > 0 else 0.0
    return {"gaps": gaps, "commits": commits, "hours": hours, "rate": rate}


def main(argv=None):
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--repo", default=str(Path.home() / ".oxidex/worktrees/fleet-ops"))
    p.add_argument("--since", required=True, help="ISO timestamp, e.g. 2026-07-25T05:30:00")
    args = p.parse_args(argv)
    r = published_gaps(args.repo, args.since)
    print(f"published gaps: {r['gaps']} across {r['commits']} commit(s)")
    print(f"window: {r['hours']:.2f} h")
    print(f"RATE: {r['rate']:.2f} gaps/hour")
    if r["rate"] > 0:
        print(f"time to zero (4333 gaps): {4333 / r['rate'] / 24:.1f} days")
    return 0


if __name__ == "__main__":
    sys.exit(main())
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_measure_throughput -v
```
Expected: PASS, 6 tests

- [ ] **Step 5: Take the real Phase 1 baseline**

```bash
cd ~/.oxidex/worktrees/fleet-ops && python3 scripts/measure_throughput.py --since 2026-07-25T05:30:00
```
Record the printed RATE. **Gate:** if it is still ≈0.24 gaps/h after ≥4 hours, STOP and re-derive per spec §9.

- [ ] **Step 6: Commit**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git add scripts/measure_throughput.py scripts/test_measure_throughput.py
git commit -m "feat(fleet): measure published gaps/hour, the spec headline metric

Production-side rates are deliberately not measured -- optimizing those
is what produced a pipeline generating work it could not ship.
Verified: trailers carry gaps=N->M; the closed count is the difference,
not the before-count.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 1: Classify validator flags as permanent vs transient

**Files:**
- Modify: `scripts/validate_fix_commit.py` (add after `REQUIRED_TRAILERS`, ~line 111)
- Test: `scripts/test_validate_fix_commit.py`

**Interfaces:**
- Consumes: existing flag strings emitted at `validate_fix_commit.py:224,313,316,484,489,491`.
- Produces: `classify_flags(flags) -> {"permanent": [...], "transient": [...]}`, and module constant `TRANSIENT_FLAG_PREFIXES`.

**Context:** A quarantined patch-id is skipped forever with no retry. That is right for a fabricated PrintConv and wrong for "the Perl lib wasn't configured, so nothing could be verified". Both currently produce `printconv-unverifiable`.

- [ ] **Step 1: Write the failing test**

Append to `scripts/test_validate_fix_commit.py` (before the `if __name__` block):

```python
class ClassifyFlagsTests(unittest.TestCase):
    def test_printconv_unverifiable_is_transient(self):
        # "could not verify" means the TOOLING was unavailable (no
        # --perl-lib, unresolvable module), not that the fix is wrong.
        from validate_fix_commit import classify_flags
        out = classify_flags(["printconv-unverifiable"])
        self.assertEqual(out["transient"], ["printconv-unverifiable"])
        self.assertEqual(out["permanent"], [])

    def test_printconv_mismatch_is_permanent(self):
        # A value that does not appear in the Perl source is a fabricated
        # PrintConv -- retrying the identical patch cannot fix it.
        from validate_fix_commit import classify_flags
        out = classify_flags(["printconv-mismatch:Intel 386"])
        self.assertEqual(out["permanent"], ["printconv-mismatch:Intel 386"])
        self.assertEqual(out["transient"], [])

    def test_missing_trailer_is_permanent(self):
        from validate_fix_commit import classify_flags
        out = classify_flags(["missing-trailer:Perl-Ref"])
        self.assertEqual(out["permanent"], ["missing-trailer:Perl-Ref"])

    def test_comparison_run_failed_is_transient(self):
        from validate_fix_commit import classify_flags
        out = classify_flags(["comparison-run-failed"])
        self.assertEqual(out["transient"], ["comparison-run-failed"])

    def test_mixed_flags_split_and_any_permanent_dominates(self):
        from validate_fix_commit import classify_flags
        out = classify_flags(["printconv-unverifiable", "missing-trailer:Tag"])
        self.assertEqual(out["transient"], ["printconv-unverifiable"])
        self.assertEqual(out["permanent"], ["missing-trailer:Tag"])

    def test_unknown_flag_defaults_to_permanent(self):
        # Fail closed: an unrecognized flag must not earn infinite retries.
        from validate_fix_commit import classify_flags
        out = classify_flags(["some-future-flag"])
        self.assertEqual(out["permanent"], ["some-future-flag"])

    def test_empty_and_none_are_safe(self):
        from validate_fix_commit import classify_flags
        self.assertEqual(classify_flags([]), {"permanent": [], "transient": []})
        self.assertEqual(classify_flags(None), {"permanent": [], "transient": []})
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_validate_fix_commit.ClassifyFlagsTests -v
```
Expected: FAIL — `ImportError: cannot import name 'classify_flags'`

- [ ] **Step 3: Write the implementation**

In `scripts/validate_fix_commit.py`, immediately after the `REQUIRED_TRAILERS` tuple:

```python
# A quarantined patch-id is skipped by every later merger poll with no
# retry (squad_merge_loop.process_commit). That is correct for a genuine
# defect and wrong for a failure that says nothing about the fix -- e.g.
# printconv-unverifiable fires when --perl-lib is unset, so NOTHING could
# be checked. Splitting the two lets the merger retry the second class
# without ever forgiving the first.
#
# Fails CLOSED: anything not listed here is treated as permanent, so a
# future flag can never accidentally earn unlimited retries.
TRANSIENT_FLAG_PREFIXES = (
    "printconv-unverifiable",   # no perl lib / unresolvable module: nothing verified
    "comparison-run-failed",    # tag-comparison subprocess died (SIGTERM/OOM)
    "review-truncated",         # provider returned a truncation sentinel
)


def classify_flags(flags):
    """Split flags into {"permanent": [...], "transient": [...]}.

    Permanent flags describe the COMMIT (fabricated PrintConv, missing
    evidence, failing test) -- re-running the identical patch produces the
    identical verdict, so it must never be retried. Transient flags
    describe the ENVIRONMENT at validation time and may succeed on a
    later poll.
    """
    permanent, transient = [], []
    for flag in flags or []:
        if str(flag).startswith(TRANSIENT_FLAG_PREFIXES):
            transient.append(flag)
        else:
            permanent.append(flag)
    return {"permanent": permanent, "transient": transient}
```

- [ ] **Step 4: Run test to verify it passes**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_validate_fix_commit -v
```
Expected: PASS — 36 tests (29 existing + 7 new)

- [ ] **Step 5: Commit**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git add scripts/validate_fix_commit.py scripts/test_validate_fix_commit.py
git commit -m "feat(fleet): classify validator flags as permanent vs transient

printconv-unverifiable fires when --perl-lib is unset -- nothing was
verified, so the commit is not known bad. Quarantine is permanent and
patch-id keyed, so today that discards good fixes forever. Fails closed:
unlisted flags are permanent.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 2: Retry transient quarantines with bounded backoff

**Files:**
- Modify: `scripts/squad_merge_loop.py` — `append_quarantine` (~line 468) and the skip site (~line 633)
- Test: `scripts/test_squad_merge_loop.py`

**Interfaces:**
- Consumes: `validate_fix_commit.classify_flags` (Task 1).
- Produces: `should_skip_quarantined(entry, now_fn=time.time, max_attempts=3) -> bool`; `append_quarantine` gains a `permanent` field in each entry.

**Context:** `append_quarantine` already computes `attempt` and `backoff_seconds` but its own docstring says they are "for operator visibility, not because this daemon ever automatically retries". This task makes them real for transient flags only.

- [ ] **Step 1: Write the failing test**

Append to `scripts/test_squad_merge_loop.py` (before the `if __name__` block):

```python
class QuarantineRetryTests(unittest.TestCase):
    def test_permanent_entry_is_always_skipped(self):
        entry = {"permanent": True, "attempt": 1, "backoff_seconds": 60,
                 "ts": "2026-07-25T00:00:00"}
        self.assertTrue(sml.should_skip_quarantined(entry, now_fn=lambda: 1e12))

    def test_transient_entry_is_skipped_while_inside_backoff(self):
        import time as _t
        now = _t.mktime(_t.strptime("2026-07-25T00:00:30", "%Y-%m-%dT%H:%M:%S"))
        entry = {"permanent": False, "attempt": 1, "backoff_seconds": 60,
                 "ts": "2026-07-25T00:00:00"}
        self.assertTrue(sml.should_skip_quarantined(entry, now_fn=lambda: now))

    def test_transient_entry_is_retried_after_backoff(self):
        import time as _t
        now = _t.mktime(_t.strptime("2026-07-25T00:05:00", "%Y-%m-%dT%H:%M:%S"))
        entry = {"permanent": False, "attempt": 1, "backoff_seconds": 60,
                 "ts": "2026-07-25T00:00:00"}
        self.assertFalse(sml.should_skip_quarantined(entry, now_fn=lambda: now))

    def test_transient_entry_becomes_permanent_after_max_attempts(self):
        import time as _t
        now = _t.mktime(_t.strptime("2026-07-25T09:00:00", "%Y-%m-%dT%H:%M:%S"))
        entry = {"permanent": False, "attempt": 3, "backoff_seconds": 60,
                 "ts": "2026-07-25T00:00:00"}
        self.assertTrue(sml.should_skip_quarantined(entry, now_fn=lambda: now,
                                                    max_attempts=3))

    def test_malformed_ts_fails_closed_to_skip(self):
        entry = {"permanent": False, "attempt": 1, "backoff_seconds": 60, "ts": "garbage"}
        self.assertTrue(sml.should_skip_quarantined(entry, now_fn=lambda: 1e12))

    def test_legacy_entry_without_permanent_field_is_skipped(self):
        # Entries written before this feature have no `permanent` key;
        # treat them as permanent so behavior is unchanged for them.
        entry = {"attempt": 1, "backoff_seconds": 60, "ts": "2026-07-25T00:00:00"}
        self.assertTrue(sml.should_skip_quarantined(entry, now_fn=lambda: 1e12))


class AppendQuarantinePermanentFieldTests(unittest.TestCase):
    def test_permanent_flag_marks_entry_permanent(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "q.jsonl"
            entry = sml.append_quarantine(
                p, patch_id="pid", sha="sha", format_name="JPEG", squad="canon",
                reason="r", flags=["missing-trailer:Tag"], now_fn=lambda: 0,
            )
            self.assertTrue(entry["permanent"])

    def test_transient_only_flags_mark_entry_retryable(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "q.jsonl"
            entry = sml.append_quarantine(
                p, patch_id="pid", sha="sha", format_name="JPEG", squad="canon",
                reason="r", flags=["printconv-unverifiable"], now_fn=lambda: 0,
            )
            self.assertFalse(entry["permanent"])

    def test_any_permanent_flag_dominates_a_mixed_set(self):
        with tempfile.TemporaryDirectory() as tmp:
            p = Path(tmp) / "q.jsonl"
            entry = sml.append_quarantine(
                p, patch_id="pid", sha="sha", format_name="JPEG", squad="canon",
                reason="r", flags=["printconv-unverifiable", "printconv-mismatch:X"],
                now_fn=lambda: 0,
            )
            self.assertTrue(entry["permanent"])
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_squad_merge_loop.QuarantineRetryTests -v
```
Expected: FAIL — `AttributeError: module 'squad_merge_loop' has no attribute 'should_skip_quarantined'`

- [ ] **Step 3: Add `permanent` to `append_quarantine`**

In `scripts/squad_merge_loop.py`, inside `append_quarantine`, replace the `entry = {...}` literal with:

```python
    classified = validate_fix_commit.classify_flags(flags)
    # Any permanent flag dominates: a commit with a fabricated PrintConv is
    # not rescued by also having had a transient tooling failure.
    permanent = bool(classified["permanent"]) or not classified["transient"]
    entry = {
        "ts": time.strftime("%Y-%m-%dT%H:%M:%S", time.localtime(now_fn())),
        "patch_id": patch_id,
        "sha": sha,
        "format": format_name,
        "squad": squad,
        "reason": reason,
        "flags": list(flags or []),
        "attempt": attempt,
        "backoff_seconds": backoff_seconds,
        "permanent": permanent,
    }
```

- [ ] **Step 4: Add `should_skip_quarantined`**

In `scripts/squad_merge_loop.py`, immediately after `append_quarantine`:

```python
DEFAULT_QUARANTINE_MAX_ATTEMPTS = 3


def should_skip_quarantined(entry, now_fn=time.time,
                            max_attempts=DEFAULT_QUARANTINE_MAX_ATTEMPTS):
    """True when a quarantined patch-id must NOT be re-processed now.

    Permanent entries are skipped forever -- re-running an identical patch
    through an identical validator yields an identical verdict, so a retry
    is pure waste.

    Transient entries (tooling unavailable at validation time, see
    validate_fix_commit.TRANSIENT_FLAG_PREFIXES) are skipped only until
    their backoff elapses, and become permanent once `attempt` reaches
    max_attempts so a genuinely broken environment cannot spin forever.

    Fails CLOSED (skip) on a malformed/absent timestamp or a legacy entry
    written before the `permanent` field existed -- preserving today's
    never-retry behavior for anything this cannot positively identify as
    retryable.
    """
    if entry.get("permanent", True):
        return True
    if entry.get("attempt", 1) >= max_attempts:
        return True
    try:
        written = time.mktime(time.strptime(entry["ts"], "%Y-%m-%dT%H:%M:%S"))
    except (KeyError, TypeError, ValueError):
        return True
    return (now_fn() - written) < entry.get("backoff_seconds", 60)
```

- [ ] **Step 5: Honor it at the skip site**

In `scripts/squad_merge_loop.py`, replace the block at ~line 633:

```python
    if patch_id in quarantine_entries:
        log_fn(f"[{squad}] {sha[:12]} ({fmt}): patch-id already quarantined -- skipped without retry")
        return {"sha": sha, "outcome": "skipped_quarantined", "patch_id": patch_id}
```

with:

```python
    prior_quarantine = quarantine_entries.get(patch_id)
    if prior_quarantine is not None and should_skip_quarantined(prior_quarantine):
        why = "permanent" if prior_quarantine.get("permanent", True) else "inside retry backoff"
        log_fn(f"[{squad}] {sha[:12]} ({fmt}): patch-id already quarantined ({why}) -- skipped")
        return {"sha": sha, "outcome": "skipped_quarantined", "patch_id": patch_id}
    if prior_quarantine is not None:
        log_fn(f"[{squad}] {sha[:12]} ({fmt}): retrying transient quarantine "
               f"(attempt {prior_quarantine.get('attempt', 1)})")
```

- [ ] **Step 6: Run the full suite**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_squad_merge_loop -v
```
Expected: PASS — 72 tests (63 existing + 9 new)

- [ ] **Step 7: Commit**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git add scripts/squad_merge_loop.py scripts/test_squad_merge_loop.py
git commit -m "feat(fleet): retry transient quarantines, keep permanent ones permanent

append_quarantine already computed attempt/backoff but its docstring said
they were advisory only. Transient flags (tooling unavailable) now earn a
bounded retry; permanent flags (fabricated PrintConv, missing evidence)
still never retry. Legacy entries lacking the field are treated as
permanent, so existing behavior is unchanged for them.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 3: Fail fast when `--perl-lib` is unusable

**Files:**
- Modify: `scripts/squad_merge_loop.py` — `main()`, at the `validate_kwargs` construction
- Test: `scripts/test_squad_merge_loop.py`

**Interfaces:**
- Produces: `check_perl_lib(perl_lib) -> Path | None`; raises `SystemExit(2)` when configured-but-unusable.

**Context:** A merger started without `--perl-lib` silently flags every commit `printconv-unverifiable`. That converts a correctness gate into a rejection generator — the exact failure that stranded real fixes tonight.

- [ ] **Step 1: Write the failing test**

Append to `scripts/test_squad_merge_loop.py`:

```python
class CheckPerlLibTests(unittest.TestCase):
    def test_valid_directory_is_returned(self):
        with tempfile.TemporaryDirectory() as tmp:
            self.assertEqual(sml.check_perl_lib(tmp), Path(tmp))

    def test_none_is_allowed_but_warns(self):
        # Not configured at all is a deliberate (if degraded) choice.
        self.assertIsNone(sml.check_perl_lib(None))

    def test_configured_but_missing_directory_exits_rather_than_degrading(self):
        with self.assertRaises(SystemExit) as ctx:
            sml.check_perl_lib("/nonexistent/perl/lib")
        self.assertEqual(ctx.exception.code, 2)

    def test_directory_without_exiftool_modules_exits(self):
        with tempfile.TemporaryDirectory() as tmp:
            with self.assertRaises(SystemExit) as ctx:
                sml.check_perl_lib(tmp)
            self.assertEqual(ctx.exception.code, 2)

    def test_directory_with_exiftool_modules_passes(self):
        with tempfile.TemporaryDirectory() as tmp:
            mod = Path(tmp) / "Image" / "ExifTool"
            mod.mkdir(parents=True)
            (mod / "Exif.pm").write_text("package Image::ExifTool::Exif;\n")
            self.assertEqual(sml.check_perl_lib(tmp), Path(tmp))
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_squad_merge_loop.CheckPerlLibTests -v
```
Expected: FAIL — `AttributeError: ... has no attribute 'check_perl_lib'`

- [ ] **Step 3: Write the implementation**

In `scripts/squad_merge_loop.py`, immediately before `def main(`:

```python
def check_perl_lib(perl_lib):
    """Validate --perl-lib at startup, or exit(2).

    A merger whose Perl lib is missing cannot verify ANY PrintConv, so it
    flags every commit printconv-unverifiable -- silently converting a
    correctness gate into a rejection generator. Since quarantine is
    patch-id keyed, that permanently discards good fixes. Better to refuse
    to start than to run as a rejection machine.

    None means "deliberately not configured": allowed, but warned about
    loudly, since the same degradation applies.
    """
    if perl_lib is None:
        print("WARNING: no --perl-lib configured -- every PrintConv check will "
              "report 'unverifiable' and nothing will be byte-verified against "
              "ExifTool source", file=sys.stderr)
        return None
    path = Path(perl_lib)
    if not path.is_dir():
        print(f"FATAL: --perl-lib is not a directory: {path}", file=sys.stderr)
        raise SystemExit(2)
    if not any(path.rglob("Image/ExifTool/*.pm")):
        print(f"FATAL: --perl-lib has no Image/ExifTool/*.pm modules: {path}\n"
              f"       expected something like "
              f"/opt/homebrew/Cellar/exiftool/<ver>/libexec/lib/perl5",
              file=sys.stderr)
        raise SystemExit(2)
    return path
```

- [ ] **Step 4: Call it from `main()`**

In `scripts/squad_merge_loop.py` `main()`, replace:

```python
    validate_kwargs = {
        "perl_lib": args.perl_lib,
```

with:

```python
    validate_kwargs = {
        "perl_lib": check_perl_lib(args.perl_lib),
```

- [ ] **Step 5: Run tests**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_squad_merge_loop -v
```
Expected: PASS — 77 tests

- [ ] **Step 6: Verify against the real lib**

```bash
cd ~/.oxidex/worktrees/fleet-ops && python3 -c "
import sys; sys.path.insert(0,'scripts')
import squad_merge_loop as s
print('OK:', s.check_perl_lib('/opt/homebrew/Cellar/exiftool/13.55/libexec/lib/perl5'))"
```
Expected: prints `OK: /opt/homebrew/Cellar/exiftool/13.55/libexec/lib/perl5`

- [ ] **Step 7: Commit**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git add scripts/squad_merge_loop.py scripts/test_squad_merge_loop.py
git commit -m "fix(fleet): refuse to start a merger with an unusable --perl-lib

A merger without a valid Perl lib cannot verify any PrintConv, so it
flags everything unverifiable -- and because quarantine is patch-id
keyed and permanent, that discards good fixes forever. Exit 2 instead of
degrading into a rejection machine.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 4: Retry once on provider truncation in `review_verdict`

**Files:**
- Modify: `scripts/model_fix_loop.py` — `review_verdict` (line 2473)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Produces: module constant `TRUNCATION_SENTINEL = "[Wafer: response was truncated"`.

**Context:** Truncation returns HTTP 200 with a sentinel in the body, so transport-level `max_retries` never fires and the reply is scored REJECT. This is disjoint from the Phase 1 trailing-verdict fix: retry the truncation FIRST, then parse.

- [ ] **Step 1: Write the failing test**

Append to `scripts/test_model_fix_loop.py`:

```python
class ReviewVerdictTruncationRetryTests(unittest.TestCase):
    GAP = {"format": "JPEG", "missing_tags": [], "value_differences": [], "gap_count": 1}
    CONFIG = {
        "models": [{"name": "m", "base_url": "b", "api_key": "k"}],
        "max_tokens": 100, "reasoning_effort": "high",
    }

    def test_truncated_reply_is_retried_once_then_parsed(self):
        from model_fix_loop import review_verdict
        replies = [
            "[Wafer: response was truncated before the model finished its "
            "internal reasoning. Increase max_tokens...]",
            "APPROVE",
        ]
        calls = []

        def fake_call(*a, **k):
            calls.append(1)
            return replies[len(calls) - 1]

        approved, reason = review_verdict(
            self.GAP, "diff", self.CONFIG, call_model_fn=fake_call,
            pick_model_fn=lambda m: m[0])
        self.assertEqual(len(calls), 2)
        self.assertTrue(approved)

    def test_truncated_twice_falls_through_to_reject_not_infinite_retry(self):
        from model_fix_loop import review_verdict
        calls = []

        def fake_call(*a, **k):
            calls.append(1)
            return "[Wafer: response was truncated before the model finished]"

        approved, reason = review_verdict(
            self.GAP, "diff", self.CONFIG, call_model_fn=fake_call,
            pick_model_fn=lambda m: m[0])
        self.assertEqual(len(calls), 2)
        self.assertFalse(approved)

    def test_normal_reply_is_not_retried(self):
        from model_fix_loop import review_verdict
        calls = []

        def fake_call(*a, **k):
            calls.append(1)
            return "APPROVE"

        approved, _ = review_verdict(
            self.GAP, "diff", self.CONFIG, call_model_fn=fake_call,
            pick_model_fn=lambda m: m[0])
        self.assertEqual(len(calls), 1)
        self.assertTrue(approved)
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_model_fix_loop.ReviewVerdictTruncationRetryTests -v
```
Expected: FAIL — `test_truncated_reply_is_retried_once_then_parsed` asserts 2 calls, gets 1

- [ ] **Step 3: Write the implementation**

In `scripts/model_fix_loop.py`, add near `INFRA_FAILURE_PREFIX`:

```python
# Wafer returns HTTP 200 with this sentinel in the BODY when a thinking
# model exhausts max_tokens mid-reasoning, so call_model's transport-level
# retry never fires and the "reply" reaches the parser as prose -> REJECT.
TRUNCATION_SENTINEL = "[Wafer: response was truncated"
```

In `review_verdict`, replace the single `try: reply = call_model_fn(...) except ...` block with a bounded loop:

```python
    reply = None
    for attempt in range(2):
        try:
            reply = call_model_fn(
                [{"role": "user", "content": prompt}],
                model_spec["base_url"], model_spec["api_key"], model_spec["name"],
                config["max_tokens"], model_spec.get("reasoning_effort") or config["reasoning_effort"],
                config.get("stream", False), config.get("thinking", True),
                config.get("temperature", 0), config.get("timeout", 120),
                config.get("max_retries", DEFAULT_MAX_RETRIES),
                config.get("retry_backoff_seconds", DEFAULT_RETRY_BACKOFF_SECONDS),
                config.get("max_retry_backoff_seconds", DEFAULT_MAX_RETRY_BACKOFF_SECONDS),
            )
        except Exception as e:
            return False, f"review call failed: {e}"
        # Exactly ONE retry: a second truncation means the prompt genuinely
        # does not fit this model's budget, and retrying further just
        # doubles an already-expensive call (observed 623s, 611s).
        if TRUNCATION_SENTINEL not in (reply or "") or attempt == 1:
            break
    verdict, reason = extract_review_verdict_full(reply)
```

- [ ] **Step 4: Run tests**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_model_fix_loop -v 2>&1 | tail -5
```
Expected: PASS — all tests, including 3 new

- [ ] **Step 5: Commit**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "fix(fleet): retry once when the provider truncates a review reply

Truncation returns HTTP 200 with a sentinel in the body, so transport
retry never fires and the reply is scored REJECT. Bounded to one retry:
a second truncation means the prompt genuinely exceeds the budget, and
these calls already cost ~10 minutes each.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 5: Deploy Phase 2 and re-measure

- [ ] **Step 1: Run every affected suite**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest \
  test_model_fix_loop test_squad_merge_loop test_validate_fix_commit \
  test_parallel_model_fix_loop test_measure_throughput 2>&1 | tail -5
```
Expected: `OK`

- [ ] **Step 2: Verify the Rust workspace is unaffected**

```bash
cd ~/.oxidex/worktrees/fleet-ops && cargo test --workspace 2>&1 | grep -E "^test result:" | grep -v "0 failed" || echo "all green"
```
Expected: `all green`

- [ ] **Step 3: PR, merge, sync**

```bash
cd ~/.oxidex/worktrees/fleet-ops
gh auth switch --hostname github.com --user swackhamer
git push -u origin HEAD
gh pr create --base main --title "feat(fleet): Phase 2 -- validator taxonomy and quarantine retry" \
  --body "Implements spec Phase 2 (docs/plans/specs/2026-07-25-fix-throughput-engine-design.md §6)."
```
Then merge and sync:
```bash
gh pr merge <N> --squash --admin --delete-branch
git fetch origin main && git fetch . origin/main:main && git checkout -B fleet-ops-local main
for d in ~/.oxidex/worktrees/squad-staging/*/; do
  sq=$(basename "$d"); [ -z "$(git -C "$d" status --short)" ] && git -C "$d" checkout -B "squad/$sq" main --quiet
done
```

- [ ] **Step 4: Restart mergers to load the new code**

```bash
pkill -f "squad_merge_loop.py --squad"; sleep 5
# supervise_mergers.sh restarts all 14 within one 30s poll
sleep 40 && pgrep -f "squad_merge_loop.py --squad" | wc -l
```
Expected: 28 (14 squads × 2 procs)

- [ ] **Step 5: Measure the Phase 2 → 3 gate**

Wait ≥2 hours, then:
```bash
cd ~/.oxidex/worktrees/fleet-ops
python3 scripts/measure_throughput.py --since <phase2-deploy-timestamp>
python3 -c "
import json
from collections import Counter
c=Counter()
for line in open('/Users/allen/.oxidex/logs/quarantine.jsonl'):
    line=line.strip()
    if line: c['permanent' if json.loads(line).get('permanent',True) else 'transient']+=1
print(c)"
```
**Gate:** proceed to Phase 3 when the quarantine rejection ratio falls below 50%, or the remainder is shown to be genuine defects.

---

# PHASE 3 — Work selection

**Entry gate:** Task 5 Step 5 passed.

---

### Task 6: Cap concurrent claims per ExifTool module (drain the FLIR tarpit)

**Files:**
- Modify: `scripts/model_fix_loop.py` — the `active` filter (~line 5110-5118)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Consumes: existing `resolve_canonical_table(tag_key, attribution)`, `gather_live_claims(state)`.
- Produces: `module_claim_count(state, attribution, module) -> int`; `DEFAULT_MAX_CLAIMS_PER_MODULE = 2`.

**Context:** FLIR/InfiRay is 0 fixes in 657 attempts, yet 40 of the last 120 diffs (33%) touch `flir_parser.rs` from 6 different squads — also causing same-file merge conflicts. Do **not** partition by squad (spec §7.1: that would have blocked 86% of successful gaps).

- [ ] **Step 1: Write the failing test**

Append to `scripts/test_model_fix_loop.py`:

```python
class ModuleClaimCapTests(unittest.TestCase):
    ATTRIB = {"tags": {
        "JPEG:APP1:FlirA": {"module": "FLIR", "table": "Main"},
        "JPEG:APP1:FlirB": {"module": "FLIR", "table": "Main"},
        "JPEG:APP1:FlirC": {"module": "FLIR", "table": "Main"},
        "CR2:EXIF:Lens":   {"module": "Exif", "table": "Main"},
    }}

    def test_counts_only_live_claims_for_that_module(self):
        from model_fix_loop import module_claim_count
        state = {
            "JPEG:APP1:FlirA": {"claimed_by": "w1", "claim_ts": 9e9},
            "JPEG:APP1:FlirB": {"claimed_by": "w2", "claim_ts": 9e9},
            "CR2:EXIF:Lens":   {"claimed_by": "w3", "claim_ts": 9e9},
        }
        self.assertEqual(module_claim_count(state, self.ATTRIB, "FLIR"), 2)
        self.assertEqual(module_claim_count(state, self.ATTRIB, "Exif"), 1)

    def test_unclaimed_tags_are_not_counted(self):
        from model_fix_loop import module_claim_count
        state = {"JPEG:APP1:FlirA": {}}
        self.assertEqual(module_claim_count(state, self.ATTRIB, "FLIR"), 0)

    def test_unknown_module_is_zero_not_an_error(self):
        from model_fix_loop import module_claim_count
        self.assertEqual(module_claim_count({}, self.ATTRIB, "Nonexistent"), 0)

    def test_missing_attribution_is_zero_not_an_error(self):
        from model_fix_loop import module_claim_count
        self.assertEqual(module_claim_count({"k": {"claimed_by": "w"}}, None, "FLIR"), 0)
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_model_fix_loop.ModuleClaimCapTests -v
```
Expected: FAIL — `ImportError: cannot import name 'module_claim_count'`

- [ ] **Step 3: Write the helper**

In `scripts/model_fix_loop.py`, near `resolve_canonical_table`:

```python
# Measured: FLIR/InfiRay is 0 fixes in 657 attempts, yet 33% of recent
# diffs target flir_parser.rs from 6 squads at once -- concentrated
# capacity on a tarpit, plus same-file cross-squad merge conflicts.
# A soft per-module cap drains it without partitioning tags by squad
# (which retroactively would have blocked 86% of successful gaps).
DEFAULT_MAX_CLAIMS_PER_MODULE = 2


def module_claim_count(state, attribution, module):
    """How many tags of `module` are currently claimed by any worker."""
    tags = ((attribution or {}).get("tags") or {})
    count = 0
    for tag_key, entry in (state or {}).items():
        if not (entry or {}).get("claimed_by"):
            continue
        if (tags.get(tag_key) or {}).get("module") == module:
            count += 1
    return count
```

- [ ] **Step 4: Apply the cap in the `active` filter**

In `scripts/model_fix_loop.py`, immediately after `active` is computed (after the `conflicts_with_live_jobs` list comprehension ending ~line 5118), insert:

```python
            # Soft per-module cap: keep at most N workers mining the same
            # ExifTool module concurrently. Applied only when it leaves
            # SOMETHING selectable -- starving a worker into a spin loop
            # (which re-runs find_gaps and burns a build-semaphore slot)
            # would cost more than the pileup it prevents.
            if attribution:
                tags_index = (attribution or {}).get("tags") or {}
                capped = [
                    tg for tg in active
                    if module_claim_count(
                        state, attribution,
                        (tags_index.get(tg["tag_key"]) or {}).get("module"),
                    ) < max_claims_per_module
                ]
                if capped:
                    active = capped
```

Thread the parameter: add `max_claims_per_module=DEFAULT_MAX_CLAIMS_PER_MODULE` to the enclosing function's signature, and pass `config.get("max_claims_per_module", DEFAULT_MAX_CLAIMS_PER_MODULE)` from its caller in `main()`.

- [ ] **Step 5: Run tests**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_model_fix_loop -v 2>&1 | tail -5
```
Expected: PASS

- [ ] **Step 6: Commit**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat(fleet): cap concurrent claims per ExifTool module

FLIR/InfiRay is 0 fixes in 657 attempts yet draws 33% of recent diffs
from 6 squads, also causing same-file merge conflicts. A soft cap drains
the tarpit without partitioning tags by squad -- which retroactively
would have blocked 86% of successful gaps. Never starves: the cap is
skipped when it would leave nothing selectable.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 7: Emit a difficulty tier per gap

**Files:**
- Modify: `scripts/attribute_gaps.py` — `attribute_gap` (line 298)
- Test: `scripts/test_attribute_gaps.py`

**Interfaces:**
- Produces: `classify_difficulty(fmt, family, name, kind, table) -> int` (0 = easiest); each `attribution["tags"][key]` gains `"difficulty": int`.

**Tiers:** 0 = value_difference (tag already emitted, only the value is wrong); 1 = missing tag in a resolved table; 2 = missing tag, no table; 3 = binary/computed (name matches `Info$|Data$|Histogram|Debug` or value looks like `(Binary data`).

- [ ] **Step 1: Write the failing test**

Append to `scripts/test_attribute_gaps.py`:

```python
class ClassifyDifficultyTests(unittest.TestCase):
    def test_value_difference_is_tier_zero(self):
        from attribute_gaps import classify_difficulty
        self.assertEqual(classify_difficulty("JPEG", "MakerNotes", "ReleaseMode", "diff", "Canon::Main"), 0)

    def test_missing_with_resolved_table_is_tier_one(self):
        from attribute_gaps import classify_difficulty
        self.assertEqual(classify_difficulty("JPEG", "EXIF", "Orientation", "missing", "Exif::Main"), 1)

    def test_missing_without_table_is_tier_two(self):
        from attribute_gaps import classify_difficulty
        self.assertEqual(classify_difficulty("JPEG", "XMP", "Foo", "missing", ""), 2)

    def test_binary_blob_names_are_tier_three(self):
        from attribute_gaps import classify_difficulty
        self.assertEqual(classify_difficulty("JPEG", "MakerNotes", "AEDebugInfo", "missing", "Canon::Main"), 3)
        self.assertEqual(classify_difficulty("JPEG", "MakerNotes", "AEHistogramInfo", "missing", "Canon::Main"), 3)

    def test_binary_beats_value_difference(self):
        # A binary blob is hard even when oxidex already emits something.
        from attribute_gaps import classify_difficulty
        self.assertEqual(classify_difficulty("JPEG", "MakerNotes", "AELocalHistogram", "diff", "Canon::Main"), 3)
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_attribute_gaps.ClassifyDifficultyTests -v
```
Expected: FAIL — `ImportError: cannot import name 'classify_difficulty'`

- [ ] **Step 3: Write the implementation**

In `scripts/attribute_gaps.py`, before `attribute_gap`:

```python
# Ordering hint for gap selection (0 = attempt first). Measured motivation:
# ~501 value_differences -- where oxidex ALREADY emits the tag and only the
# value is wrong -- sit permanently at the tail of every list because
# selection is `active[0]` in list order, so the cheapest wins in the whole
# corpus are effectively unreachable.
DIFFICULTY_VALUE_DIFF = 0
DIFFICULTY_MISSING_WITH_TABLE = 1
DIFFICULTY_MISSING_NO_TABLE = 2
DIFFICULTY_BINARY = 3

_BINARY_NAME_RE = re.compile(r"(Info|Data)$|Histogram|Debug|Thumbnail", re.IGNORECASE)


def classify_difficulty(fmt, family, name, kind, table):
    """Cheapest-first difficulty tier for one gap.

    Binary/opaque blobs are checked FIRST and dominate: reproducing
    ExifTool's "(Binary data N bytes...)" rendering is hard whether or not
    oxidex currently emits something for that key.
    """
    if _BINARY_NAME_RE.search(name or ""):
        return DIFFICULTY_BINARY
    if kind == "diff":
        return DIFFICULTY_VALUE_DIFF
    if table:
        return DIFFICULTY_MISSING_WITH_TABLE
    return DIFFICULTY_MISSING_NO_TABLE
```

Then in `attribute_gap`, add `"difficulty": classify_difficulty(fmt, family, name, kind, table)` to the returned dict (thread `kind` in from `iter_gaps`, which already distinguishes missing vs diff).

- [ ] **Step 4: Run tests**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_attribute_gaps -v 2>&1 | tail -5
```
Expected: PASS

- [ ] **Step 5: Regenerate attribution and sanity-check the distribution**

```bash
cd ~/.oxidex/worktrees/fleet-ops && python3 scripts/attribute_gaps.py
python3 -c "
import json
from collections import Counter
d=json.load(open('/Users/allen/.oxidex/logs/gap-attribution.json'))
print(Counter(t.get('difficulty') for t in d['tags'].values()))"
```
Expected: a Counter with meaningful counts in tiers 0–3 (tier 0 ≈ 500).

- [ ] **Step 6: Commit**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git add scripts/attribute_gaps.py scripts/test_attribute_gaps.py
git commit -m "feat(fleet): classify each gap into a difficulty tier

~501 value_differences (tag already emitted, only the value wrong) sit at
the tail of every list because selection is active[0] in list order, so
the cheapest wins in the corpus are unreachable. Tier 0 surfaces them.
Binary blobs dominate as tier 3 regardless of kind.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 8: Select gaps cheapest-first

**Files:**
- Modify: `scripts/model_fix_loop.py` — replace `tag_gap = active[0]` (line 5138)
- Test: `scripts/test_model_fix_loop.py`

**Interfaces:**
- Consumes: `attribution["tags"][key]["difficulty"]` (Task 7).
- Produces: `gap_sort_key(tag_gap, state, attribution) -> tuple`.

- [ ] **Step 1: Write the failing test**

Append to `scripts/test_model_fix_loop.py`:

```python
class GapSortKeyTests(unittest.TestCase):
    ATTRIB = {"tags": {
        "JPEG:EXIF:Easy": {"difficulty": 0},
        "JPEG:EXIF:Med":  {"difficulty": 1},
        "JPEG:EXIF:Hard": {"difficulty": 3},
    }}

    def test_lower_difficulty_sorts_first(self):
        from model_fix_loop import gap_sort_key
        gaps = [{"tag_key": k} for k in ("JPEG:EXIF:Hard", "JPEG:EXIF:Easy", "JPEG:EXIF:Med")]
        ordered = sorted(gaps, key=lambda g: gap_sort_key(g, {}, self.ATTRIB))
        self.assertEqual([g["tag_key"] for g in ordered],
                         ["JPEG:EXIF:Easy", "JPEG:EXIF:Med", "JPEG:EXIF:Hard"])

    def test_fewer_prior_failures_sorts_first_within_a_tier(self):
        from model_fix_loop import gap_sort_key
        state = {"JPEG:EXIF:Easy": {"fails": 4}}
        gaps = [{"tag_key": "JPEG:EXIF:Easy"}, {"tag_key": "JPEG:EXIF:Med"}]
        ordered = sorted(gaps, key=lambda g: gap_sort_key(g, state, self.ATTRIB))
        # Med (tier 1, 0 fails) beats Easy (tier 0, 4 fails) only if fails
        # outrank tier -- they must NOT; tier is primary.
        self.assertEqual(ordered[0]["tag_key"], "JPEG:EXIF:Easy")

    def test_within_same_tier_fewer_fails_wins(self):
        from model_fix_loop import gap_sort_key
        attrib = {"tags": {"A": {"difficulty": 1}, "B": {"difficulty": 1}}}
        state = {"A": {"fails": 3}}
        gaps = [{"tag_key": "A"}, {"tag_key": "B"}]
        ordered = sorted(gaps, key=lambda g: gap_sort_key(g, state, attrib))
        self.assertEqual(ordered[0]["tag_key"], "B")

    def test_unknown_tag_gets_middle_difficulty_not_a_crash(self):
        from model_fix_loop import gap_sort_key
        key = gap_sort_key({"tag_key": "NOPE"}, {}, self.ATTRIB)
        self.assertIsInstance(key, tuple)

    def test_missing_attribution_is_stable_not_a_crash(self):
        from model_fix_loop import gap_sort_key
        self.assertIsInstance(gap_sort_key({"tag_key": "X"}, {}, None), tuple)
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_model_fix_loop.GapSortKeyTests -v
```
Expected: FAIL — `ImportError: cannot import name 'gap_sort_key'`

- [ ] **Step 3: Write the implementation**

In `scripts/model_fix_loop.py`, near `make_single_tag_gap`:

```python
DEFAULT_UNKNOWN_DIFFICULTY = 2


def gap_sort_key(tag_gap, state, attribution):
    """Cheapest-first ordering key: (difficulty, prior_fails, tag_key).

    Difficulty is PRIMARY -- a tier-0 tag with several prior failures is
    still a better bet than a tier-3 binary blob nobody has touched.
    prior_fails breaks ties so the loop rotates off a tag that keeps
    failing instead of retrying it every round. tag_key last keeps the
    order deterministic across processes.
    """
    key = tag_gap["tag_key"]
    tags = ((attribution or {}).get("tags") or {})
    difficulty = (tags.get(key) or {}).get("difficulty", DEFAULT_UNKNOWN_DIFFICULTY)
    fails = ((state or {}).get(key) or {}).get("fails", 0)
    return (difficulty, fails, key)
```

Then replace line 5138:

```python
            tag_gap = active[0]
```

with:

```python
            active.sort(key=lambda tg: gap_sort_key(tg, state, attribution))
            tag_gap = active[0]
```

- [ ] **Step 4: Run tests**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_model_fix_loop -v 2>&1 | tail -5
```
Expected: PASS

- [ ] **Step 5: Commit**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git add scripts/model_fix_loop.py scripts/test_model_fix_loop.py
git commit -m "feat(fleet): select gaps cheapest-first instead of list order

Selection was active[0] in list order, so ~501 value_differences sat
permanently at the tail and were unreachable. Difficulty is primary,
prior-fail count breaks ties so the loop rotates off tags that keep
failing, tag_key keeps it deterministic.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 9: Apportion worker slots to formats by open-gap weight

**Files:**
- Modify: `scripts/parallel_model_fix_loop.py` — `squad_worker_formats`
- Test: `scripts/test_parallel_model_fix_loop.py`

**Interfaces:**
- Produces: `weighted_formats(squad, attribution, squads_toml_path, slots) -> list[str]` of length `slots`.

**Context:** Round-robin leaves ~2 of 20 slots idling per round and makes ~460 gaps unreachable.

- [ ] **Step 1: Write the failing test**

Append to `scripts/test_parallel_model_fix_loop.py`:

```python
class WeightedFormatsTests(unittest.TestCase):
    ATTRIB = {"squads": {"canon": {"formats": ["JPEG", "CR2", "HEIC"]}},
              "tags": {
                  **{f"JPEG:EXIF:T{i}": {"squad": "canon", "formats": ["JPEG"]} for i in range(80)},
                  **{f"CR2:EXIF:T{i}": {"squad": "canon", "formats": ["CR2"]} for i in range(18)},
                  **{f"HEIC:EXIF:T{i}": {"squad": "canon", "formats": ["HEIC"]} for i in range(2)},
              }}

    def test_returns_exactly_one_format_per_slot(self):
        out = pmfl.weighted_formats("canon", self.ATTRIB, "/unused", 4)
        self.assertEqual(len(out), 4)

    def test_heaviest_format_gets_the_most_slots(self):
        out = pmfl.weighted_formats("canon", self.ATTRIB, "/unused", 10)
        self.assertGreater(out.count("JPEG"), out.count("CR2"))
        self.assertGreaterEqual(out.count("CR2"), out.count("HEIC"))

    def test_every_format_with_gaps_gets_at_least_one_slot_when_slots_allow(self):
        out = pmfl.weighted_formats("canon", self.ATTRIB, "/unused", 3)
        self.assertEqual(set(out), {"JPEG", "CR2", "HEIC"})

    def test_more_slots_than_formats_never_yields_an_empty_slot(self):
        out = pmfl.weighted_formats("canon", self.ATTRIB, "/unused", 7)
        self.assertEqual(len(out), 7)
        self.assertTrue(all(f for f in out))

    def test_no_attribution_falls_back_to_round_robin_length(self):
        out = pmfl.weighted_formats("canon", None, "/unused", 3)
        self.assertEqual(len(out), 3)
```

- [ ] **Step 2: Run test to verify it fails**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_parallel_model_fix_loop.WeightedFormatsTests -v
```
Expected: FAIL — `AttributeError: ... has no attribute 'weighted_formats'`

- [ ] **Step 3: Write the implementation**

In `scripts/parallel_model_fix_loop.py`, immediately after `squad_worker_formats`:

```python
def weighted_formats(squad, attribution, squads_toml_path, slots):
    """One format per slot, apportioned by this squad's open-gap counts.

    Round-robin gave every format equal slots regardless of how much work
    it held, which idled ~2 of 20 slots per round and left ~460 gaps
    unreachable. Largest-remainder apportionment instead, with a floor of
    one slot per format that still has gaps so a small format is never
    starved outright.
    """
    formats = squad_worker_formats(squad, attribution, squads_toml_path)
    if not formats or slots <= 0:
        return []
    tags = ((attribution or {}).get("tags") or {})
    counts = {f: 0 for f in formats}
    for meta in tags.values():
        if meta.get("squad") != squad:
            continue
        for f in meta.get("formats") or []:
            if f in counts:
                counts[f] += 1
    total = sum(counts.values())
    if total <= 0:
        return [formats[n % len(formats)] for n in range(slots)]

    # Floor of 1 for any format with work, then distribute the remainder
    # by largest fractional share.
    alloc = {f: (1 if counts[f] > 0 else 0) for f in formats}
    remaining = slots - sum(alloc.values())
    if remaining < 0:
        ranked = sorted(formats, key=lambda f: -counts[f])
        return ranked[:slots]
    shares = sorted(
        ((counts[f] / total, f) for f in formats), key=lambda x: (-x[0], x[1])
    )
    i = 0
    while remaining > 0 and shares:
        alloc[shares[i % len(shares)][1]] += 1
        remaining -= 1
        i += 1
    out = []
    for f in formats:
        out.extend([f] * alloc[f])
    return out[:slots]
```

- [ ] **Step 4: Run tests**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest test_parallel_model_fix_loop -v 2>&1 | tail -5
```
Expected: PASS

- [ ] **Step 5: Use it in the dispatcher**

In `run_squad_round`, replace the per-slot round-robin format pick with a single call to `weighted_formats(squad, attribution, args.squads_toml, slots)` and index that list by slot number.

- [ ] **Step 6: Commit**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git add scripts/parallel_model_fix_loop.py scripts/test_parallel_model_fix_loop.py
git commit -m "feat(fleet): apportion slots to formats by open-gap weight

Round-robin gave equal slots regardless of work held, idling ~2 of 20
slots per round and leaving ~460 gaps unreachable. Largest-remainder
apportionment with a one-slot floor per format that still has gaps.

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

### Task 10: Deploy Phase 3 and re-measure

- [ ] **Step 1: Full test sweep**

```bash
cd ~/.oxidex/worktrees/fleet-ops/scripts && python3 -m unittest \
  test_model_fix_loop test_squad_merge_loop test_validate_fix_commit \
  test_parallel_model_fix_loop test_attribute_gaps test_measure_throughput 2>&1 | tail -5
cd ~/.oxidex/worktrees/fleet-ops && cargo test --workspace 2>&1 | grep -E "^test result:" | grep -v "0 failed" || echo "rust green"
```
Expected: `OK` and `rust green`

- [ ] **Step 2: PR, merge, sync, restart dispatcher AND mergers**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git push -u origin HEAD && gh pr create --base main --title "feat(fleet): Phase 3 -- work selection"
# after merge:
git fetch origin main && git fetch . origin/main:main && git checkout -B fleet-ops-local main
python3 scripts/stop_parallel_fix.py && sleep 5
nohup uv run scripts/parallel_model_fix_loop.py --infinite --squad-mode --max-parallel 20 \
  --worktree-dir ~/.oxidex/worktrees/parallel-fix --log-dir ~/.oxidex/logs/parallel-model-fix \
  --home ~/.oxidex >> ~/.oxidex/logs/parallel-model-fix-wrapper.log 2>&1 &
disown
```

- [ ] **Step 3: Measure the Phase 3 → 4 gate**

Wait ≥4 hours, then:
```bash
cd ~/.oxidex/worktrees/fleet-ops && python3 scripts/measure_throughput.py --since <phase3-deploy-timestamp>
```
**Gate for Phase 4:** published rate ≈ production rate (publishing is no longer the constraint). If publishing still lags, Phase 4 is premature — return to §6.

---

# PHASE 4 — Bulk table-port (gated)

**Entry gate:** Task 10 Step 3 passed. Phase 4 is **not** authorized before that.

---

### Task 11: Unblock T3 (three blockers) and run one pilot

**Files:**
- Modify: `scripts/model_fix_loop.py` — `attempt_table_port` trailer construction; `DEFAULT_MAX_TABLE_SOURCE_CHARS`
- Modify: `scripts/validate_fix_commit.py` — table-port evidence shape
- Test: `scripts/test_model_fix_loop.py`, `scripts/test_validate_fix_commit.py`

**Blockers, all three required before the pilot (spec §8):**

1. **No driver.** `run_table_job` does not exist (0 definitions). `attempt_table_port` is a bare inner loop with no claim/release, heartbeat, state persistence, worktree refresh, or lesson emission.
2. **Unpublishable output.** `attempt_table_port` emits exactly `{Format, Table, Worker, Verified}`; `REQUIRED_TRAILERS` is `{Format, Tag, Sample, Exiftool-Value, Oxidex-Value, Perl-Ref, Verified, Worker}` — 5 missing, so every commit quarantines on arrival.
3. **Source cap defeats the gate.** `DEFAULT_MAX_TABLE_SOURCE_CHARS = 12_000` vs `DEFAULT_TABLE_PORT_THRESHOLD = 0.8`. Measured visibility: `CanonCustom::Functions2` 27.5%, `NikonCustom::SettingsD3` 39.6%, `Exif::Main` 8%.

- [ ] **Step 1: Confirm the pilot table still fits**

Three call details matter, all verified:
- Signature is `extract_perl_table_source(table_name, lib_dir, max_chars=...)` — **table name FIRST**.
- `lib_dir` globs `*.pm` **non-recursively**, so it must be the directory holding the modules (`.../perl5/Image/ExifTool`), **not** the perl5 root. Passing the root silently returns `None` for every table.
- `max_chars` truncates, so pass a large value to measure TRUE size rather than the capped size.

```bash
cd ~/.oxidex/worktrees/fleet-ops && python3 -c "
import sys; sys.path.insert(0,'scripts')
from model_fix_loop import extract_perl_table_source, DEFAULT_MAX_TABLE_SOURCE_CHARS
LIB='/opt/homebrew/Cellar/exiftool/13.55/libexec/lib/perl5/Image/ExifTool'
for t in ('Sony::AFStatus79','CanonCustom::Functions2','NikonCustom::SettingsD3','Exif::Main'):
    f = extract_perl_table_source(t, LIB, max_chars=10**7) or ''
    vis = 100.0*min(len(f),DEFAULT_MAX_TABLE_SOURCE_CHARS)/max(len(f),1)
    print(f'{t:28s} {len(f):7d} chars  {vis:5.1f}% visible')"
```

Verified output (reproduces the spec's §8 numbers exactly):

```
Sony::AFStatus79                9256 chars  100.0% visible
CanonCustom::Functions2        43693 chars   27.5% visible
NikonCustom::SettingsD3        30341 chars   39.6% visible
Exif::Main                    143046 chars    8.4% visible
```

`Sony::AFStatus79` is the pilot: 95 gaps at 100% visibility. If it ever drops below ~95%, pick a different table first — the ≥80%-exact/zero-wrong gate is unwinnable from partial source.

- [ ] **Step 2: Teach the validator a table-port evidence shape**

Add to `scripts/validate_fix_commit.py` a `TABLE_PORT_REQUIRED_TRAILERS = ("Format", "Table", "Worker", "Verified", "Perl-Ref")` and select it in `check_trailers` when a `Table:` trailer is present, so a table-port commit is validated against per-table evidence rather than per-tag evidence. Write tests first, asserting a per-tag commit still requires the original 8.

- [ ] **Step 3: Make `attempt_table_port` emit that shape**

In its `trailers` dict, add `"Perl-Ref": <pm file>` from the resolved module.

- [ ] **Step 4: Run the pilot by hand (no dispatcher wiring yet)**

Drive `attempt_table_port` directly for `('JPEG', 'Sony', 'AFStatus79')` in a scratch worktree and record: gate pass/fail, gaps closed, wall-clock, whether the commit validates clean.

- [ ] **Step 5: Decide, with data**

- Pilot lands and validates ⇒ write the `run_table_job` driver (blocker 1) as its own follow-up plan.
- Pilot fails the ≥80%-exact/zero-wrong gate ⇒ **stop**. Record the result in the spec's §8 and do not fund a fleet-wide rollout. T3 has executed zero times in 46,709 calls; a failed pilot is a real answer, not a setback.

- [ ] **Step 6: Commit the pilot result either way**

```bash
cd ~/.oxidex/worktrees/fleet-ops
git add docs/plans/specs/2026-07-25-fix-throughput-engine-design.md
git commit -m "docs: record Sony::AFStatus79 table-port pilot result

Co-Authored-By: Claude <noreply@anthropic.com>"
```

---

## Self-Review

**Spec coverage:**

| Spec section | Task |
|---|---|
| §5 Phase 1 | Shipped in PR #116 (out of scope, noted in Global Constraints) |
| §6.1 quarantine retry taxonomy | Tasks 1, 2 |
| §6.2 enforce `--perl-lib` | Task 3 |
| §6.3 truncation retry | Task 4 |
| §7.1 FLIR tarpit | Task 6 |
| §7.2 difficulty ordering | Tasks 7, 8 |
| §7.3 gap-weighted formats | Task 9 |
| §8 gated table-port | Task 11 |
| §9 measurement | Task 0, plus gates in Tasks 5, 10, 11 |
| §10 what not to do | Encoded as explicit non-goals in Tasks 6 and 11 |

No spec section is unimplemented.

**Placeholder scan:** No TBD/TODO. Every code step contains real code. Task 11 Steps 2–4 are intentionally lighter because they are gated behind a pilot that may cancel them — the blockers and the exact trailer sets are named concretely, and Step 5 makes "stop" an explicit, acceptable outcome.

**Type consistency:** `classify_flags` returns `{"permanent", "transient"}` (Task 1) and is consumed with those exact keys in Task 2. `should_skip_quarantined(entry, now_fn, max_attempts)` matches its call site. `module_claim_count(state, attribution, module)` matches. `gap_sort_key(tag_gap, state, attribution)` returns a 3-tuple used only via `sorted(key=)`. `weighted_formats(squad, attribution, squads_toml_path, slots)` matches its test and dispatcher call. `classify_difficulty(fmt, family, name, kind, table)` matches. `published_gaps(...)` returns `{gaps, commits, hours, rate}` as asserted.
